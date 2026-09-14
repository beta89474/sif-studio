import zipfile, os, hashlib

src_dir = r'C:\Users\Beta\WorkBuddy\2026-09-12-09-46-53\sif-studio-release\sif-studio-v0.1.0-portable'
out = os.path.join(src_dir.rsplit('\\', 1)[0], 'sif-studio-v0.1.0-portable.zip')

# Final structure: zip = exe + README only; CHECKSUM.txt lives outside zip
with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
    for f in ['README.txt', 'sif-studio.exe']:
        full = os.path.join(src_dir, f)
        z.write(full, arcname=f)
        print(f'  in zip: {f} ({os.path.getsize(full)} B)')

with open(out, 'rb') as f:
    zip_hash = hashlib.sha256(f.read()).hexdigest()
with open(os.path.join(src_dir, 'sif-studio.exe'), 'rb') as f:
    exe_hash = hashlib.sha256(f.read()).hexdigest()
print(f'zip sha256: {zip_hash}')
print(f'exe sha256: {exe_hash}')
print(f'zip size:   {os.path.getsize(out)} B')

checksum_text = (
    "SHA256 Checksums\n"
    "================\n"
    "\n"
    f"{zip_hash}  sif-studio-v0.1.0-portable.zip\n"
    f"{exe_hash}  sif-studio.exe\n"
    "\n"
    "验证方法 (PowerShell):\n"
    "  Get-FileHash .\\sif-studio-v0.1.0-portable.zip -Algorithm SHA256\n"
    "  Get-FileHash .\\sif-studio.exe -Algorithm SHA256\n"
)
with open(os.path.join(src_dir, 'CHECKSUM.txt'), 'w', encoding='utf-8') as f:
    f.write(checksum_text)
print('CHECKSUM.txt updated with final hashes')
print(f'zip final size: {os.path.getsize(out)} B')