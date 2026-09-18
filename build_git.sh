#!/bin/bash
# 镜像 build.bat 的构建流程（cmd.exe 在沙箱中禁用，改用 Git Bash 运行）
cd "$(dirname "$0")" || exit 1
export PATH="/c/msys64/mingw64/bin:$PATH"

echo "[1/5] version stamp"
# version_stamp.py 默认以 cwd 为 ROOT 打戳；复制脚本到本地避免 $HOME 路径在原生 python 下被误转。
cp /c/Users/Administrator/.workbuddy/scripts/version_stamp.py ./_version_stamp.py || \
  cp "$HOME/.workbuddy/scripts/version_stamp.py" ./_version_stamp.py
python3 ./_version_stamp.py || { echo "ERROR: version_stamp"; exit 1; }
rm -f ./_version_stamp.py

echo "[2/5] WinSuiteXShell.dll"
windres -i shell.rc -o shellres.o || { echo "ERROR: shell.rc"; exit 1; }
g++ -O2 -Wall -ffunction-sections -fdata-sections -DBUILD_SHELL_DLL -shared \
    -finput-charset=UTF-8 -DUNICODE -D_UNICODE \
    explorercommand.cpp shellres.o \
    -o WinSuiteXShell.dll \
    -lshell32 -lshlwapi -ladvapi32 -luser32 -lole32 -luuid \
    -static -Wl,--kill-at -Wl,--dynamicbase,--nxcompat -s || { echo "ERROR: dll"; exit 1; }
rm -f shellres.o

echo "[3/5] res.o (内嵌 DLL)"
windres -i app.rc -o res.o || { echo "ERROR: app.rc"; exit 1; }

echo "[3.5/5] libcpuid (.o)"
for F in cpuid_main recog_intel recog_amd libcpuid_util asm-bits; do
  gcc -O2 -std=gnu11 -DHAVE_CONFIG_H -Ithirdparty/libcpuid -c thirdparty/libcpuid/$F.c -o thirdparty/libcpuid/$F.o || { echo "ERROR: libcpuid $F"; exit 1; }
done

echo "[4/5] WinSuiteX.exe"
g++ -O2 -Wall -ffunction-sections -fdata-sections -mwindows -municode \
    -DUNICODE -D_UNICODE -finput-charset=UTF-8 \
    -Ithirdparty/libcpuid \
    main.cpp features.cpp selftest.cpp hardware.cpp res.o \
    thirdparty/libcpuid/cpuid_main.o thirdparty/libcpuid/recog_intel.o thirdparty/libcpuid/recog_amd.o thirdparty/libcpuid/libcpuid_util.o thirdparty/libcpuid/asm-bits.o \
    -o build_main.exe \
    -lshell32 -lshlwapi -ladvapi32 -luser32 -lole32 -luuid \
    -lsetupapi -liphlpapi -lkernel32 -ldxgi -ldxguid -lgdiplus \
    -static -Wl,--kill-at -Wl,--dynamicbase,--nxcompat -Wl,--gc-sections -s || { echo "ERROR: link"; exit 1; }

rm -f res.o thirdparty/libcpuid/*.o

echo "[5/5] rename"
python3 rename_outputs.py || { echo "ERROR: rename"; exit 1; }

echo "BUILD OK"
ls -la WinSuiteX.exe
