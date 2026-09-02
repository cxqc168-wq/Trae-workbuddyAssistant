import zipfile, os

base = r"D:\ai_work\trae-work-assistant\release\Trae Work 助手"
zip_path = r"D:\ai_work\trae-work-assistant\release\Trae Work 助手_2.4.4_x64_portable.zip"

if os.path.exists(zip_path):
    os.remove(zip_path)

parent = os.path.dirname(base)
count = 0
with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as z:
    for root, dirs, files in os.walk(base):
        for f in files:
            full = os.path.join(root, f)
            rel = os.path.relpath(full, parent)  # "Trae Work 助手/..."
            z.write(full, rel)
            count += 1
print("created", zip_path, os.path.getsize(zip_path), "files:", count)
