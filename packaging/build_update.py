"""Small code/assets update for existing compatible 1.0.x portable installations."""
import hashlib
import json
from pathlib import Path
import zipfile

from build_release import ROOT,VERSION,STAGE


def main():
    if not (STAGE/'VietKaraStudio.exe').is_file():raise SystemExit('Build --app-only first.')
    output=ROOT/'releases'/f'VietKaraStudio-{VERSION}-Update.zip'
    files=[p for folder in ('app','dependencies') for p in (STAGE/folder).rglob('*')
           if p.is_file() and 'bin' not in p.relative_to(STAGE).parts and '__pycache__' not in p.parts and p.suffix not in ('.pyc','.pyo')]
    files += [STAGE/name for name in ('VietKaraStudio.exe','bootstrap.py','BUILD-INFO.json')]
    guide=f'''VIETKARA STUDIO {VERSION} - CAP NHAT PORTABLE 1.0.0 / 1.0.1

1. Dong VietKara Studio truoc khi cap nhat.
2. Giai nen NOI DUNG ZIP nay de ghi de vao thu muc portable cu dang co
   VietKaraStudio.exe, runtime/ va app/. Chap nhan thay the file trung ten.
3. Mo lai VietKaraStudio.exe. Chan trang hien {VERSION}.

GOI NAY CHI LA BAN CAP NHAT, KHONG PHAI BO CAI DOC LAP.
Khong xoa hoac thay the data/: project, bai nhac va model cu duoc giu nguyen.
Khong giai nen vao thu muc chua VocalCreationLab.exe: hay chon dung thu muc VietKara Studio.

Tinh nang:
- Bong chu duoc giu den khi ca cau bien mat.
- Beat cua ban / Ban co vocal nam tren banner.
- Bam loa tai ba hang song nhac de nghe Beat, ban goc, Vocal rieng hoac tat tieng.
- Tach video ngay trong Karaoke, nap beat va hai nguon can loi vao project.
- Tieu de bai hat/ca si, font/mau/co chu, thoi gian, hieu ung vao/ra va mau san.
'''
    manifest={str(p.relative_to(STAGE)).replace('\\','/'):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    with zipfile.ZipFile(output.with_suffix('.partial'),'w',zipfile.ZIP_DEFLATED,compresslevel=6) as z:
        for p in files:z.write(p,p.relative_to(STAGE))
        z.writestr('HUONG-DAN-CAP-NHAT.txt',guide)
        z.writestr('UPDATE-MANIFEST.json',json.dumps({'version':VERSION,'files':manifest},indent=2))
    output.with_suffix('.partial').replace(output)
    with zipfile.ZipFile(output) as z:
        assert z.testzip() is None
        assert not any(n.startswith(('data/','runtime/')) for n in z.namelist())
        for name,digest in manifest.items():assert hashlib.sha256(z.read(name)).hexdigest()==digest
    with output.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
    output.with_suffix('.sha256').write_text(digest+'  '+output.name+'\n',encoding='ascii')
    print(json.dumps({'file':str(output),'bytes':output.stat().st_size,'files':len(manifest),'verified':True}))


if __name__=='__main__':main()
