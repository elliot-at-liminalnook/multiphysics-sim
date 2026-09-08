// Capture drawn canvas frames only. Encoding is opt-in and is not a physics
// benchmark or a substitute for the saved input recording.
export function installVideoExport(canvas, button, name) {
  let recorder = null, stream = null;
  const supported = typeof MediaRecorder !== 'undefined' && typeof canvas.captureStream === 'function';
  button.disabled = !supported;
  if (!supported) button.title = 'Canvas video export is unavailable in this browser.';
  function stop() { if (recorder?.state === 'recording') recorder.stop(); }
  button.onclick = () => {
    if (recorder) { stop(); return; }
    const mimeType = ['video/webm;codecs=vp9', 'video/webm;codecs=vp8', 'video/webm'].find(t => MediaRecorder.isTypeSupported(t));
    const filename = `${name()}-${Date.now()}.webm`, chunks = [];
    try {
      stream = canvas.captureStream(30); recorder = new MediaRecorder(stream, mimeType ? {mimeType} : {});
      recorder.ondataavailable = e => { if (e.data.size) chunks.push(e.data); };
      recorder.onstop = () => {
        const type = recorder.mimeType;
        stream.getTracks().forEach(t => t.stop()); stream = null; recorder = null;
        button.textContent = 'Record video'; button.setAttribute('aria-pressed', 'false');
        if (chunks.length) {
          const url = URL.createObjectURL(new Blob(chunks, {type})), a = document.createElement('a');
          a.href = url; a.download = filename; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
        }
      };
      recorder.start(1000); button.textContent = 'Stop video & save'; button.setAttribute('aria-pressed', 'true');
    } catch (e) { stream?.getTracks().forEach(t => t.stop()); stream = null; recorder = null; button.title = e.message; button.textContent = 'Video unavailable'; }
  };
  return {stop};
}
