"""Bounded, cancellable MP4 export: Qt captures; a worker encodes frames."""
import math
import os
import queue
import subprocess
import tempfile
import threading
from copy import deepcopy
from pathlib import Path
from PySide6.QtCore import QTimer, Qt
from PySide6.QtGui import QImage, QPainter, QColor
from PySide6.QtWidgets import QPushButton,QComboBox,QDoubleSpinBox,QSlider,QPlainTextEdit,QCheckBox


def video_canvas(source, width, height):
    """Composite physical framebuffer pixels, independent of screen DPI."""
    source = source.copy()
    # QOpenGLWidget's framebuffer carries the screen's device pixel ratio.
    # QPainter otherwise draws the scaled image at its logical (half) size
    # on Retina screens, despite the offsets being calculated in pixels.
    source.setDevicePixelRatio(1.)
    scaled=source.scaled(width,height,Qt.KeepAspectRatio,Qt.SmoothTransformation)
    canvas=QImage(width,height,QImage.Format_RGB888);canvas.fill(QColor('#202429'))
    painter=QPainter(canvas)
    painter.drawImage((width-scaled.width())//2,(height-scaled.height())//2,scaled)
    painter.end()
    return canvas


class MotionVideo:
    def __init__(self, panel):
        self.panel=panel;self.running=False;self.status='idle';self.error=None;self.path=None
        self.timer=QTimer(panel);self.timer.setInterval(10);self.timer.timeout.connect(self.step)
        self.frame=0;self.total=0

    def state(self):
        return {'running':self.running,'status':self.status,'path':str(self.path) if self.path else None,
                'frames':self.frame,'total_frames':self.total,'error':self.error}

    def start(self,path,fps=24,width=1280,height=720):
        if self.running: raise ValueError('A video export is already running')
        if type(fps) is not int or not 1<=fps<=60: raise ValueError('Frame rate must be an integer from 1 to 60')
        if any(type(v) is not int or not 64<=v<=3840 or v%2 for v in (width,height)): raise ValueError('Video dimensions must be even integers from 64 to 3840')
        dest=Path(path).expanduser().absolute()
        if dest.suffix.lower()!='.mp4': raise ValueError('Choose an .mp4 output path')
        from imageio_ffmpeg import get_ffmpeg_exe
        executable=get_ffmpeg_exe()
        p=self.panel;p.prepare();p.pause()
        self.program=deepcopy(p.program);self.revision=p.app.doc.revision
        self.saved_positions=dict(p.positions);self.saved_time=p.playhead
        self.camera=deepcopy(p.app.viewport.camera)
        self.frame=0;self.total=max(1,round(self.program['duration']*fps))
        self.fps=fps;self.width=width;self.height=height;self.path=dest
        self.error=None;self.status='rendering';self.cancelled=threading.Event();self.done=threading.Event()
        self.restore=True;self.frames=queue.Queue(maxsize=2)
        dest.parent.mkdir(parents=True,exist_ok=True)
        fd,temp=tempfile.mkstemp(prefix='.'+dest.stem+'-',suffix='.mp4',dir=dest.parent);os.close(fd);self.temp=Path(temp)
        self.log=tempfile.TemporaryFile()
        try:
            self.process=subprocess.Popen([executable,'-hide_banner','-loglevel','error','-y',
                '-f','rawvideo','-pix_fmt','rgb24','-s',f'{width}x{height}','-r',str(fps),'-i','pipe:0',
                '-an','-c:v','libx264','-preset','fast','-crf','20','-pix_fmt','yuv420p','-movflags','+faststart',str(self.temp)],
                stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=self.log)
        except Exception:
            self.temp.unlink(missing_ok=True);self.log.close();raise
        self.running=True
        self.widgets={w:w.isEnabled() for cls in (QPushButton,QComboBox,QDoubleSpinBox,QSlider,QPlainTextEdit,QCheckBox) for w in p.findChildren(cls)}
        for w in self.widgets:w.setEnabled(False)
        p.cancel_video.setEnabled(True)
        p.app.viewport.setEnabled(False)
        self.thread=threading.Thread(target=self.encode,daemon=True);self.thread.start();self.timer.start()
        return self.state()

    def encode(self):
        try:
            while not self.cancelled.is_set():
                try:data=self.frames.get(timeout=.1)
                except queue.Empty:continue
                if data is None:break
                self.process.stdin.write(data)
            self.process.stdin.close()
            if self.cancelled.is_set():
                self.process.terminate();self.process.wait(timeout=5)
            elif self.process.wait(timeout=60)!=0:
                self.log.seek(0);raise RuntimeError(self.log.read().decode(errors='replace')[-1500:] or 'Video encoder failed')
            # Publication belongs to Qt after completion, so Cancel wins until
            # the finished event is handled. Existing destination stays intact.
        except Exception as error:
            if not self.cancelled.is_set():self.error=str(error)
        finally:
            if self.process.poll() is None:self.process.kill();self.process.wait()
            self.log.close();self.done.set()

    def step(self):
        if not self.running:return
        if self.done.is_set():self.finish();return
        p=self.panel
        if p.app.doc.revision!=self.revision:
            self.error='Document changed during export';self.cancel(restore=False);return
        if self.cancelled.is_set() or self.frames.full():return
        if self.frame>=self.total:
            if self.status!='encoding':self.frames.put_nowait(None);self.status='encoding'
            return
        try:
            p.app.viewport.camera=deepcopy(self.camera)
            p.seek(self.frame/self.fps)
            # The OpenGL framebuffer excludes sidebar/tool/annotation widgets.
            source=p.app.viewport.grabFramebuffer()
            if source.isNull():raise RuntimeError('Viewport did not provide a video frame')
            canvas=video_canvas(source,self.width,self.height)
            raw=bytes(canvas.constBits())
            stride=canvas.bytesPerLine();row=self.width*3
            if stride!=row:raw=b''.join(raw[y*stride:y*stride+row] for y in range(self.height))
            self.frames.put_nowait(raw);self.frame+=1
            p.video_progress.setText(f'Exporting {self.frame}/{self.total} frames ({100*self.frame/self.total:.0f}%)')
        except Exception as error:self.error=str(error);self.cancel()

    def cancel(self,restore=True):
        if not self.running:return
        self.restore=restore;self.cancelled.set();self.status='cancelling'
        # Interrupt a blocked pipe write as well as a waiting worker.
        if self.process.poll() is None:self.process.terminate()

    def finish(self):
        self.timer.stop();self.running=False;p=self.panel
        success=not self.cancelled.is_set() and not self.error
        if success:
            try:os.replace(self.temp,self.path)
            except OSError as error:self.error=str(error);success=False
        if not success:self.temp.unlink(missing_ok=True)
        self.status='complete' if success else 'failed' if self.error else 'cancelled'
        for w,enabled in self.widgets.items():w.setEnabled(enabled)
        p.cancel_video.setEnabled(False);p.app.viewport.setEnabled(True)
        if self.restore and p.active and p.app.doc.revision==self.revision:
            p.positions=self.saved_positions;p.apply();p.playhead=self.saved_time
            p.timeline.blockSignals(True);p.timeline.setValue(round(1000*self.saved_time/self.program['duration']));p.timeline.blockSignals(False)
            p.clock.setText(f'{self.saved_time:.2f} / {self.program["duration"]:.2f} s')
        message=f'Saved video: {self.path}' if success else self.error or 'Video export cancelled'
        p.video_progress.setText(message);p.app.status(message)
