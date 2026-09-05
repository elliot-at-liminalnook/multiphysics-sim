"""Extract inspection frames. Requires Pillow and imageio-ffmpeg (or FFMPEG_EXE)."""
import hashlib,json,os,subprocess
from pathlib import Path
from PIL import Image,ImageDraw
root=Path(__file__).resolve().parent
binary=os.environ.get('FFMPEG_EXE')
if not binary:
 import imageio_ffmpeg
 binary=imageio_ffmpeg.get_ffmpeg_exe()
report=[]
for clip in sorted(root.glob('*.mp4')):
 folder=root/'frames'/clip.stem;folder.mkdir(parents=True,exist_ok=True)
 p=subprocess.run([binary,'-hide_banner','-y','-i',str(clip),'-vf','fps=2','-q:v','2',str(folder/'%03d.jpg')],capture_output=True,text=True,check=True)
 (root/f'{clip.stem}-metadata.txt').write_text(p.stderr)
 frames=sorted(folder.glob('*.jpg'));w=240;h=456
 sheet=Image.new('RGB',(w*5,h*((len(frames)+4)//5)),(28,30,34));draw=ImageDraw.Draw(sheet)
 for n,path in enumerate(frames):
  im=Image.open(path);im.thumbnail((w,h-30));x=n%5*w;y=n//5*h
  sheet.paste(im,(x,y+28));draw.text((x+6,y+6),f'{clip.stem} ~{n/2:.1f}s',fill='white')
 sheet.save(root/f'{clip.stem}-contact-sheet.jpg',quality=93)
 report.append({'file':clip.name,'sha256':hashlib.sha256(clip.read_bytes()).hexdigest(),'sampling_fps':2,'frames':[str(f.relative_to(root)) for f in frames]})
 print(clip.name,len(frames),'frames',flush=True)
(root/'index.json').write_text(json.dumps(report,indent=2))
