import urllib.request
import zipfile
import os
import sys

url = "https://github.com/protocolbuffers/protobuf/releases/download/v27.5/protoc-27.5-win64.zip"
zip_path = os.path.join(os.path.dirname(__file__), "protoc_win.zip")
exe_path = os.path.join(os.path.dirname(__file__), "protoc.exe")

print(f"Downloading protoc from {url}...")
try:
    urllib.request.urlretrieve(url, zip_path)
    print(f"Downloaded to {zip_path}")
except Exception as e:
    print(f"Download failed: {e}")
    sys.exit(1)

print(f"Extracting protoc.exe...")
try:
    with zipfile.ZipFile(zip_path, 'r') as z:
        z.extractall(os.path.dirname(__file__))
    print(f"Extracted to {os.path.dirname(__file__)}")
except Exception as e:
    print(f"Extraction failed: {e}")
    sys.exit(1)

if os.path.exists(exe_path):
    print(f"protoc.exe found at {exe_path}")
    print("SUCCESS")
else:
    # Check if it's in bin/ subdirectory
    alt_path = os.path.join(os.path.dirname(__file__), "bin", "protoc.exe")
    if os.path.exists(alt_path):
        import shutil
        shutil.copy(alt_path, exe_path)
        print(f"Copied from {alt_path} to {exe_path}")
        print("SUCCESS")
    else:
        print(f"protoc.exe not found after extraction!")
        sys.exit(1)
