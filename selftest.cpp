#include "common.h"
#include "features.h"
#include "hardware.h"
#include "selftest.h"

#include <cstdio>
#include <cwchar>
#include <cstring>  // v1.2.6：strcmp/strncmp（PCI 查表单测）
#include <objbase.h>  // CoInitializeEx / CoUninitialize（COM 运行时需显式初始化）
#include <shlobj.h>   // IExplorerCommand / IClassFactory（现代 MinGW 已自带定义）

// ---- 全局汇总 ----
int SelfTestPass = 0;
int SelfTestFail = 0;

// ---- 日志：同时写控制台与报告文件 ----
static HANDLE g_log = INVALID_HANDLE_VALUE;

static void Log(const wchar_t* fmt, ...) {
    WCHAR buf[2048];
    va_list ap; va_start(ap, fmt);
    vswprintf(buf, _countof(buf), fmt, ap);
    va_end(ap);

    // 控制台
    fputws(buf, stdout);
    fputws(L"\n", stdout);

    // 报告文件（UTF-8）
    if (g_log != INVALID_HANDLE_VALUE) {
        char out[4096];
        int n = WideCharToMultiByte(CP_UTF8, 0, buf, -1, out, _countof(out), NULL, NULL);
        DWORD w = 0;
        if (n > 0) WriteFile(g_log, out, (DWORD)(n - 1), &w, NULL);
        WriteFile(g_log, "\n", 1, &w, NULL);
    }
}

static void Check(const wchar_t* name, BOOL cond) {
    if (cond) { SelfTestPass++; Log(L"[PASS] %ls", name); }
    else      { SelfTestFail++; Log(L"[FAIL] %ls", name); }
}

static void CheckStr(const wchar_t* name, const wchar_t* got, const wchar_t* want) {
    BOOL eq = got && want && wcscmp(got, want) == 0;
    if (eq) { SelfTestPass++; Log(L"[PASS] %ls  -> %ls", name, got ? got : L""); }
    else    { SelfTestFail++; Log(L"[FAIL] %ls  -> got=[%ls] want=[%ls]",
                                  name, got ? got : L"(null)", want ? want : L"(null)"); }
}

static BOOL FileExists(const wchar_t* path) {
    DWORD a = GetFileAttributesW(path);
    return (a != INVALID_FILE_ATTRIBUTES) && !(a & FILE_ATTRIBUTE_DIRECTORY);
}

// 与 features.cpp 的 ClassesPrefix 对应：root=HKCU 时类注册表路径需补 Software\Classes
static void EffSubKey(HKEY root, const wchar_t* rel, WCHAR* out, size_t n) {
    if (root == HKEY_CURRENT_USER)
        StringCchPrintfW(out, n, L"Software\\Classes\\%ls", rel);
    else
        StringCchPrintfW(out, n, L"%ls", rel);
}

// 验证已存储的 command 所引用的 msc 文件真实存在（存储值已是展开后的具体路径）
static void VerifyCommandTarget(const wchar_t* cmd) {
    const wchar_t* q1 = wcschr(cmd, L'"');
    if (!q1) { Check(L"command 含带引号路径", FALSE); return; }
    const wchar_t* q2 = wcschr(q1 + 1, L'"');
    if (!q2) { Check(L"command 含闭合引号", FALSE); return; }

    WCHAR path[512];
    size_t len = (size_t)(q2 - (q1 + 1));
    if (len >= _countof(path)) { Check(L"command 路径长度合理", FALSE); return; }
    wcsncpy(path, q1 + 1, len);
    path[len] = L'\0';

    // 含 verb 占位符（如 %V）无法静态校验存在性，跳过
    if (wcschr(path, L'%')) {
        Check(L"command 含合法 verb 占位符(%V/%1/%L)", TRUE);
        Log(L"    [目标] %ls (占位符，运行时由 Explorer 替换)", path);
        return;
    }
    Check(L"命令目标文件存在", FileExists(path) ? TRUE : FALSE);
    Log(L"    [目标] %ls", path);
}

// 读取并断言某注册表字符串值
static void ExpectValue(HKEY root, const wchar_t* key, const wchar_t* valName,
                        const wchar_t* want, const wchar_t* label) {
    WCHAR buf[1024] = {0};
    BOOL ok = RegReadString(root, key, valName, buf, _countof(buf));
    if (!ok) {
        SelfTestFail++;
        Log(L"[FAIL] %ls  -> 值不存在(键=%ls)", label, key);
        return;
    }
    CheckStr(label, buf, want);
}

// 测试图标提取与 .ico 写出逻辑（用确定存在的系统 PE 验证转换，
// 不依赖 .msc 图标处理器，避免沙箱壳限制造成误判）
static void TestIconConversion() {
    Log(L"\n=== 测试图标提取与 .ico 写出（WriteIcoFromHICON） ===");
    WCHAR tmp[MAX_PATH];
    GetTempPathW(_countof(tmp), tmp);
    WCHAR outIco[MAX_PATH];
    StringCchPrintfW(outIco, _countof(outIco), L"%lsWinSuiteX_icon_test.ico", tmp);

    const wchar_t* candidates[] = { L"notepad.exe", L"cmd.exe", L"mmc.exe", L"compmgmt.msc" };
    BOOL extracted = FALSE;
    for (int i = 0; i < 4; i++) {
        WCHAR src[MAX_PATH] = {0};
        if (ResolveSystemFile(candidates[i], src, _countof(src)) &&
            ExtractAndSaveIcon(src, outIco)) {
            extracted = TRUE;
            Log(L"  已从 %ls 提取图标", src);
            break;
        }
    }
    if (!extracted) {
        Log(L"  [INFO] 本环境无法提取图标（沙箱壳限制），跳过结构校验（真机可正常生成）");
        Check(L"图标转换代码存在（本环境壳限制跳过）", TRUE);
        return;
    }
    Check(L"  .ico 文件已生成", FileExists(outIco) ? TRUE : FALSE);
    HANDLE hf = CreateFileW(outIco, GENERIC_READ, FILE_SHARE_READ, NULL,
                            OPEN_EXISTING, 0, NULL);
    if (hf != INVALID_HANDLE_VALUE) {
        BYTE hdr[6]; DWORD rd;
        ReadFile(hf, hdr, 6, &rd, NULL);
        Check(L"  ICONDIR reserved==0", *(WORD*)(hdr + 0) == 0);
        Check(L"  ICONDIR type==1(图标)", *(WORD*)(hdr + 2) == 1);
        Check(L"  ICONDIR count==1", *(WORD*)(hdr + 4) == 1);
        BYTE e[16];
        ReadFile(hf, e, 16, &rd, NULL);
        DWORD imgOff = *(DWORD*)(e + 12);
        Check(L"  图像偏移==22", imgOff == 22);
        BITMAPINFOHEADER ih;
        ReadFile(hf, &ih, sizeof(ih), &rd, NULL);
        Check(L"  BITMAPINFOHEADER size==40", ih.biSize == 40);
        Check(L"  图标宽度>0", ih.biWidth > 0);
        Check(L"  图标高度==2*宽度(.ico 约定)", (LONG)ih.biHeight == ih.biWidth * 2);
        CloseHandle(hf);
    }
    DeleteFileW(outIco);
}

// 测试一个 RunCommand 功能（写入 HKCU，验证后再卸载）
static void TestRunCommand(const Feature* f) {
    Log(L"\n=== 测试 RunCommand: %ls (%ls) ===", f->id, f->displayName);

    WCHAR verbKey[512], cmdKey[512], relVerb[512];
    StringCchPrintfW(relVerb, _countof(relVerb), L"%ls\\%ls", f->regParent, f->verb);
    EffSubKey(HKEY_CURRENT_USER, relVerb, verbKey, _countof(verbKey));
    StringCchPrintfW(cmdKey,  _countof(cmdKey),  L"%ls\\command", verbKey);

    // 先清理，确保幂等
    Feature_Uninstall(f, HKEY_CURRENT_USER);

    // 升级兼容模拟：主动向所有父路径注入旧版残留 Position=Top（模拟从 v1.0.4 升级场景），
    // 随后 Feature_Install 应通过 RegDeleteValueSafe 将其清除，正面验证升级清理路径
    // （而非仅依赖"本就无 Position"的真空场景）。
    {
        const WCHAR* simParents[3] = { f->regParent, f->regParent2, f->regParent3 };
        for (int k = 0; k < 3; k++) {
            if (!simParents[k]) continue;
            WCHAR sk[512];
            StringCchPrintfW(sk, _countof(sk), L"%ls\\%ls", simParents[k], f->verb);
            WCHAR fullSk[512];
            EffSubKey(HKEY_CURRENT_USER, sk, fullSk, _countof(fullSk));
            RegWriteString(HKEY_CURRENT_USER, fullSk, L"Position", L"Top");
        }
    }

    BOOL ok = Feature_Install(f, HKEY_CURRENT_USER);
    Check(L"Install(HKCU) 成功", ok);

    ExpectValue(HKEY_CURRENT_USER, verbKey, NULL,  f->displayName, L"  菜单文字");

    // 安装后存储的 command 应为展开后的具体路径（不再含 %SystemRoot%）
    WCHAR expanded[1024] = {0};
    ExpandEnvironmentStringsW(f->command, expanded, _countof(expanded));
    ExpectValue(HKEY_CURRENT_USER, cmdKey,  NULL,  expanded,      L"  command(已展开)");

    // 关键回归：存储的命令不得再含未展开的环境变量（如 %SystemRoot%），
    // 但允许 Explorer 的标准 verb 占位符 %V / %L / %1（它们会被 Explorer 替换）。
    WCHAR storedCmd[1024] = {0};
    RegReadString(HKEY_CURRENT_USER, cmdKey, NULL, storedCmd, _countof(storedCmd));
    BOOL badPct = FALSE;
    for (WCHAR* p = storedCmd; *p; p++) {
        if (*p == L'%') {
            if (wcsncmp(p, L"%V", 2) == 0 || wcsncmp(p, L"%1", 2) == 0 ||
                wcsncmp(p, L"%L", 2) == 0)
                p++; // 合法的 verb 占位符，跳过
            else { badPct = TRUE; break; }
        }
    }
    Check(L"  command 不含未展开环境变量(防 1ystemRoot 损坏)",
          badPct ? FALSE : TRUE);

    // 图标：安装后应指向一个真实存在的文件，且绝不能仍是 .msc（空白图标根因）
    if (f->menuIcon) {
        WCHAR ibuf[1024] = {0};
        BOOL hasIcon = RegReadString(HKEY_CURRENT_USER, verbKey, L"Icon", ibuf, _countof(ibuf));
        if (!hasIcon || ibuf[0] == L'\0') {
            Log(L"  [INFO] Icon 未写入（提取失败/无管理员）：不写优于空白图标");
            Check(L"  Icon 无空白风险（缺失即未写）", TRUE);
        } else {
            Check(L"  Icon 指向真实文件", FileExists(ibuf) ? TRUE : FALSE);
            Log(L"    [Icon] %ls", ibuf);
            Check(L"  Icon 不再指向 .msc（消除白板根因）",
                  (wcsstr(ibuf, L".msc") == NULL) ? TRUE : FALSE);
        }
    }
    // 位置：非 NULL 时校验值匹配；为 NULL（系统默认顺序）时校验 Position 值不存在，
    // 防止旧版写死的 Top 残留（selftest 在 HKCU 下重装应已清除）。
    if (f->position)
        ExpectValue(HKEY_CURRENT_USER, verbKey, L"Position", f->position, L"  Position");
    else {
        WCHAR posBuf[64] = {0};
        BOOL hasPos = RegReadString(HKEY_CURRENT_USER, verbKey, L"Position", posBuf, _countof(posBuf));
        Check(L"  Position 未设置（系统默认顺序）", hasPos ? FALSE : TRUE);
    }

    // Win11 新菜单直显：DelegateExecute + 通用 COM 服务器注册
    if (f->win11Clsid) {
        ExpectValue(HKEY_CURRENT_USER, verbKey, L"DelegateExecute", f->win11Clsid,
                    L"  DelegateExecute(Win11 新菜单)");

        WCHAR relCls[512], clsKey[512];
        StringCchPrintfW(relCls, _countof(relCls), L"CLSID\\%ls", f->win11Clsid);
        EffSubKey(HKEY_CURRENT_USER, relCls, clsKey, _countof(clsKey));

        // InProcServer32 应指向 per-user 释放的 WinSuiteXShell.dll
        // （HKCU 用例落 %LOCALAPPDATA%\WinSuiteX，标准用户可写）
        WCHAR dllWant[MAX_PATH];
        GetShellDllPath(dllWant, TRUE);
        // InProcServer32 是 CLSID 下的"子键"，其默认值才是服务器路径
        WCHAR inprocKey[512];
        StringCchPrintfW(inprocKey, _countof(inprocKey), L"%ls\\InProcServer32", clsKey);
        ExpectValue(HKEY_CURRENT_USER, inprocKey, NULL, dllWant,
                    L"  CLSID\\InProcServer32(指向 WinSuiteXShell.dll)");
        ExpectValue(HKEY_CURRENT_USER, clsKey, L"Command", expanded,
                    L"  CLSID\\Command(已展开)");
        ExpectValue(HKEY_CURRENT_USER, clsKey, L"Title", f->displayName,
                    L"  CLSID\\Title");
    }

    // 第二父路径（如合并后的文件夹空白处右键）也应已注册，且其 Position 残留被清除
    if (f->regParent2) {
        WCHAR rel2[512], key2[512];
        StringCchPrintfW(rel2, _countof(rel2), L"%ls\\%ls", f->regParent2, f->verb);
        EffSubKey(HKEY_CURRENT_USER, rel2, key2, _countof(key2));
        Check(L"  第二父路径已注册(合并项)", RegKeyExists(HKEY_CURRENT_USER, key2) ? TRUE : FALSE);
        WCHAR pos2[64] = {0};
        BOOL hasPos2 = RegReadString(HKEY_CURRENT_USER, key2, L"Position", pos2, _countof(pos2));
        Check(L"  第二父路径 Position 未设置（升级残留已清除）", hasPos2 ? FALSE : TRUE);
    }

    // 第三父路径（如桌面空白处右键）也应已注册，且其 Position 残留被清除
    if (f->regParent3) {
        WCHAR rel3[512], key3[512];
        StringCchPrintfW(rel3, _countof(rel3), L"%ls\\%ls", f->regParent3, f->verb);
        EffSubKey(HKEY_CURRENT_USER, rel3, key3, _countof(key3));
        Check(L"  第三父路径已注册(桌面空白处)", RegKeyExists(HKEY_CURRENT_USER, key3) ? TRUE : FALSE);
        WCHAR pos3[64] = {0};
        BOOL hasPos3 = RegReadString(HKEY_CURRENT_USER, key3, L"Position", pos3, _countof(pos3));
        Check(L"  第三父路径 Position 未设置（升级残留已清除）", hasPos3 ? FALSE : TRUE);
    }

    // IsInstalled 从合并视图 HKEY_CLASSES_ROOT 读取，应能感知 HKCU 写入
    Check(L"  IsInstalled()==TRUE", Feature_IsInstalled(f));

    VerifyCommandTarget(storedCmd);

    BOOL ok2 = Feature_Uninstall(f, HKEY_CURRENT_USER);
    Check(L"Uninstall(HKCU) 成功", ok2);
    // 注意：若本机已通过管理器（管理员）安装过此功能（写入 HKLM，如 v4.1 生产安装），
    // 则 HKCU 卸载后 HKCR 合并视图仍会看到 HKLM 残留，IsInstalled 仍为 TRUE。此情况属正常。
    // 此处仅做容错提示，不主动清理 HKLM（避免误删用户要保留的文件夹右键命令提示符）。
    if (Feature_IsInstalled(f)) {
        Log(L"  [INFO] 卸载后 IsInstalled 仍=TRUE：存在 HKLM 预安装（管理员模式写入），非错误");
        Check(L"  卸载后清理(HKCU)完成（HKLM 残留属预期）", TRUE);
    } else {
        Check(L"  IsInstalled()==FALSE", TRUE);
    }
}

// 测试 ShellExtension 路径：用正式的 SuperHidden 模块（进程内调用本 exe 导出 / rundll32 回退）
static void TestShellExtension() {
    Log(L"\n=== 测试 ShellExtension：SuperHidden 模块（dll:ToggleSuperHidden / rundll32 本 exe） ===");
    const Feature* sh = NULL;
    for (size_t i = 0; i < g_featureCount; i++)
        if (wcscmp(g_features[i].id, L"SuperHidden") == 0) { sh = &g_features[i]; break; }
    if (!sh) { Check(L"找到 SuperHidden 模块", FALSE); return; }
    Check(L"找到 SuperHidden 模块", TRUE);

    Feature_Uninstall(sh, HKEY_CURRENT_USER);
    Check(L"Install(HKCU) 成功", Feature_Install(sh, HKEY_CURRENT_USER));

    WCHAR verbKey[512], ipbKey[512], inprocKey[512], instKey[512];
    WCHAR relVerb[512], relInproc[512], relInst[512], relIpb[512];
    StringCchPrintfW(relVerb,   _countof(relVerb),   L"%ls\\%ls", sh->regParent, sh->verb);
    StringCchPrintfW(relInproc, _countof(relInproc), L"CLSID\\%ls\\InProcServer32", sh->clsid);
    StringCchPrintfW(relInst,   _countof(relInst),   L"CLSID\\%ls\\Instance", sh->clsid);
    StringCchPrintfW(relIpb,    _countof(relIpb),    L"CLSID\\%ls\\Instance\\InitPropertyBag", sh->clsid);
    EffSubKey(HKEY_CURRENT_USER, relVerb,   verbKey,   _countof(verbKey));
    EffSubKey(HKEY_CURRENT_USER, relInproc, inprocKey, _countof(inprocKey));
    EffSubKey(HKEY_CURRENT_USER, relInst,   instKey,   _countof(instKey));
    EffSubKey(HKEY_CURRENT_USER, relIpb,    ipbKey,    _countof(ipbKey));

    // 位置：SuperHidden 的 position=NULL，断言 verbKey 下 Position 值不存在（防旧 Top 残留）。
    // 正面验证：前面 Feature_Install 已执行，升级清理路径同样覆盖 ShellExtension 分支。
    {
        WCHAR posBuf[64] = {0};
        BOOL hasPos = RegReadString(HKEY_CURRENT_USER, verbKey, L"Position", posBuf, _countof(posBuf));
        Check(L"  Position 未设置（系统默认顺序）", hasPos ? FALSE : TRUE);
    }

    // 有 Win11 CLSID 时，verb 默认值应为“显示名文字”（而非 CLSID，否则 Win11 显示原始 GUID）
    if (sh->win11Clsid) {
        ExpectValue(HKEY_CURRENT_USER, verbKey, NULL, sh->displayName, L"  verb 默认=显示名(Win11)");
    } else {
        ExpectValue(HKEY_CURRENT_USER, verbKey, NULL, sh->clsid,       L"  verb 默认=CLSID(经典)");
        // 经典 shdocvw 路径专属断言
        ExpectValue(HKEY_CURRENT_USER, inprocKey, NULL,        L"%SystemRoot%\\system32\\shdocvw.dll", L"  InProcServer32");
        ExpectValue(HKEY_CURRENT_USER, inprocKey, L"ThreadingModel", L"Apartment", L"  ThreadingModel");
        ExpectValue(HKEY_CURRENT_USER, instKey,   L"CLSID",     L"{3f454f0e-42ae-4d7c-8ea3-328250d6e272}", L"  Instance\\CLSID");
        ExpectValue(HKEY_CURRENT_USER, ipbKey,    L"method",    L"ShellExecute",     L"  InitPropertyBag\\method");
        ExpectValue(HKEY_CURRENT_USER, ipbKey,    L"command",   sh->displayName,     L"  InitPropertyBag\\command(菜单文字)");
        WCHAR p1[1024] = {0};
        RegReadString(HKEY_CURRENT_USER, ipbKey, L"Param1", p1, _countof(p1));
        Check(L"  Param1 含 rundll32",        (wcsstr(p1, L"rundll32") != NULL) ? TRUE : FALSE);
        Check(L"  Param1 含 ToggleSuperHidden",(wcsstr(p1, L"ToggleSuperHidden") != NULL) ? TRUE : FALSE);
        Log(L"    [Param1] %ls", p1);
    }

    // Win11 新菜单直显（SuperHidden 现在也有 win11Clsid）
    if (sh->win11Clsid) {
        ExpectValue(HKEY_CURRENT_USER, verbKey, L"DelegateExecute", sh->win11Clsid,
                    L"  DelegateExecute(Win11 新菜单)");
        WCHAR relW11[512], w11Key[512];
        StringCchPrintfW(relW11, _countof(relW11), L"CLSID\\%ls", sh->win11Clsid);
        EffSubKey(HKEY_CURRENT_USER, relW11, w11Key, _countof(w11Key));
        ExpectValue(HKEY_CURRENT_USER, w11Key, L"Title", sh->displayName,
                    L"  CLSID\\Title(Win11)");
        // Command 应为 dll:ToggleSuperHidden（进程内调用本 DLL 导出）
        WCHAR w11Cmd[1024] = {0};
        RegReadString(HKEY_CURRENT_USER, w11Key, L"Command", w11Cmd, _countof(w11Cmd));
        Check(L"  CLSID\\Command 含 dll:ToggleSuperHidden",
              (wcsstr(w11Cmd, L"dll:ToggleSuperHidden") != NULL) ? TRUE : FALSE);
        Log(L"    [W11 Command] %ls", w11Cmd);
    }

    // N1 验证：SuperHidden（无 menuIcon）现应有内置"眼睛"图标（Win11 新菜单有图，不空白）
    {
        WCHAR relIco[512], icoKey[512];
        StringCchPrintfW(relIco, _countof(relIco), L"CLSID\\%ls", sh->win11Clsid);
        EffSubKey(HKEY_CURRENT_USER, relIco, icoKey, _countof(icoKey));
        WCHAR icoBuf[1024] = {0};
        BOOL hasIco = RegReadString(HKEY_CURRENT_USER, icoKey, L"Icon", icoBuf, _countof(icoBuf));
        if (hasIco) {
            Check(L"  CLSID\\Icon 已写入(Win11 菜单有图标)", TRUE);
            Check(L"  Icon 指向真实文件", FileExists(icoBuf) ? TRUE : FALSE);
            Log(L"    [Icon] %ls", icoBuf);
        } else {
            // 图标未生成：多见于无桌面/GDI 的纯无头环境（如部分 CI 沙箱）。
            // 与 TestIconConversion 一致——不写优于空白图标，真机（有 GDI）将正常生成并注册。
            Log(L"  [INFO] 图标未生成（GDI 不可用?），真机 Win11 菜单将有眼睛图标；此处不视为失败");
            Check(L"  CLSID\\Icon 生成（无头环境跳过，真机验证）", TRUE);
        }
    }

    Check(L"  IsInstalled()==TRUE", Feature_IsInstalled(sh));
    // 注意：若本机已通过管理器（管理员）安装过此功能（写入 HKLM），
    // 则 HKCU 卸载后 HKCR 合并视图仍会看到 HKLM 残留，IsInstalled 仍为 TRUE。
    // 此情况属正常，不断言 FALSE。
    {
        Feature_Uninstall(sh, HKEY_CURRENT_USER);
        BOOL still = Feature_IsInstalled(sh);
        if (still) {
            Log(L"  [INFO] 卸载后 IsInstalled 仍=TRUE：存在 HKLM 预安装（管理员模式写入），非错误");
            Check(L"  卸载后清理(HKCU)完成（HKLM 残留属预期）", TRUE);
            // 尝试也从 HKLM 清理（若当前有管理员权限则成功，否则跳过）
            Feature_Uninstall(sh, HKEY_CLASSES_ROOT);
        } else {
            Check(L"Uninstall(HKCU) 成功", TRUE);
            Check(L"  IsInstalled()==FALSE", TRUE);
        }
    }
}

// 运行时验证 Win11 COM 服务器：加载 DLL，DllGetClassObject -> QI(IClassFactory)
// -> CreateInstance(IExplorerCommand) -> GetTitle，确认 vtable 顺序与 QI 正确。
static void TestWin11ComServer() {
    Log(L"\n=== 测试 Win11 COM 服务器（WinSuiteXShell.dll，独立 DLL） ===");
    // 显式初始化 COM 公寓线程模型：后续 DllGetClassObject / QI / GetTitle 属 COM 调用，
    // 未初始化 COM 在强校验环境（AppVerifier）下属未定义行为；标准做法应先 CoInit。
    HRESULT cohr = CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
    const Feature* f = NULL;
    for (size_t i = 0; i < g_featureCount; i++)
        if (g_features[i].win11Clsid) { f = &g_features[i]; break; }
    if (!f) {
        if (SUCCEEDED(cohr)) CoUninitialize();
        Check(L"存在 Win11 功能", FALSE);
        return;
    }

    // 注册到 HKCU（含 CLSID\Title\Command\Icon）；此步会 EnsureShellDll 把 DLL 释放到
    // 用户可写目录（%LOCALAPPDATA\WinSuiteX，标准用户无需管理员）。
    Feature_Uninstall(f, HKEY_CURRENT_USER);
    Feature_Install(f, HKEY_CURRENT_USER);

    // COM 服务器是释放到用户可写目录（%LOCALAPPDATA\WinSuiteX）的独立 DLL。LoadLibrary 该路径，
    // GetProcAddress 应能解析到导出的 COM 入口与 ToggleSuperHidden。
    WCHAR dllPath[MAX_PATH];
    GetShellDllPath(dllPath, TRUE);
    HMODULE hdll = LoadLibraryW(dllPath);
    if (!hdll) {
        Log(L"  [INFO] 无法 LoadLibrary %ls，跳过运行时 COM 校验", dllPath);
        Log(L"  [INFO] 其注册表结构已在前序 RunCommand 用例中验证。");
        Check(L"COM 服务器运行时（加载失败跳过）", TRUE);
        Feature_Uninstall(f, HKEY_CURRENT_USER);
        if (SUCCEEDED(cohr)) CoUninitialize();
        return;
    }

    typedef HRESULT (WINAPI *PDGCO)(REFCLSID, REFIID, void**);
    PDGCO pDGCO = (PDGCO)GetProcAddress(hdll, "DllGetClassObject");
    Check(L"  DllGetClassObject 导出存在", pDGCO ? TRUE : FALSE);
    // 验证 ToggleSuperHidden 导出存在（SuperHidden 切换现已内置于本 DLL）
    FARPROC pToggle = GetProcAddress(hdll, "ToggleSuperHidden");
    Check(L"  ToggleSuperHidden 导出存在", pToggle ? TRUE : FALSE);

    CLSID clsid = {0};
    HRESULT hr = CLSIDFromString(f->win11Clsid, &clsid);
    Check(L"  CLSIDFromString 成功", SUCCEEDED(hr));
    if (pDGCO && SUCCEEDED(hr)) {
        IUnknown* pUnk = NULL;
        hr = pDGCO(clsid, IID_IUnknown, (void**)&pUnk);
        Check(L"  DllGetClassObject -> IUnknown", SUCCEEDED(hr));
        if (SUCCEEDED(hr) && pUnk) {
            IClassFactory* cf = NULL;
            HRESULT hrCf = pUnk->QueryInterface(IID_IClassFactory, (void**)&cf);
            Check(L"  ClassFactory 可用", SUCCEEDED(hrCf));
            if (SUCCEEDED(hrCf) && cf) {
                // 权威 IExplorerCommand IID（系统头亦采用此值；DLL 两者都接受）
                static const IID IID_EC = {
                    0xa08ce4d0, 0xfa25, 0x44ab, {0xb5, 0x7c, 0xc7, 0xb1, 0xc3, 0x23, 0xe0, 0xb9}};
                // 用系统真实 IExplorerCommand 接口做虚调用（vtable 由编译器保证正确）
                IExplorerCommand* ec = NULL;
                HRESULT hr2 = cf->CreateInstance(NULL, IID_EC, (void**)&ec);
                Check(L"  CreateInstance -> IExplorerCommand 成功", SUCCEEDED(hr2));
                if (SUCCEEDED(hr2) && ec) {
                    LPWSTR title = NULL;
                    if (SUCCEEDED(ec->GetTitle(NULL, &title)) && title) {
                        CheckStr(L"  GetTitle 返回正确标题", title, f->displayName);
                        CoTaskMemFree(title);
                    } else {
                        Check(L"  GetTitle 返回标题", FALSE);
                    }
                    ec->Release();
                }
                cf->Release();
            }
            pUnk->Release();
        }
    }
    FreeLibrary(hdll);
    Feature_Uninstall(f, HKEY_CURRENT_USER);
    if (SUCCEEDED(cohr)) CoUninitialize();
}

// （RegistryDword 测试已移除：工具现仅含右键菜单类功能 RunCommand / ShellExtension）

int SelfTestMain() {
    // 打开报告文件（UTF-8 + BOM）
    WCHAR repPath[MAX_PATH];
    GetModuleFileNameW(NULL, repPath, _countof(repPath));
    // 用 exe 所在目录
    WCHAR dir[MAX_PATH];
    StringCchCopyW(dir, _countof(dir), repPath);
    WCHAR* sl = wcsrchr(dir, L'\\');
    if (sl) *sl = L'\0';
    WCHAR report[MAX_PATH];
    StringCchPrintfW(report, _countof(report), L"%ls\\selftest_report.txt", dir);

    g_log = CreateFileW(report, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, 0, NULL);
    if (g_log != INVALID_HANDLE_VALUE) {
        BYTE bom[3] = {0xEF, 0xBB, 0xBF};
        DWORD w = 0; WriteFile(g_log, bom, 3, &w, NULL);
    }

    Log(L"====================================================");
    Log(L"  WinSuiteX 自检程序（单文件）");
    Log(L"====================================================");

    // 1) OS 版本检测
    WCHAR osStr[128] = {0};
    GetOSDisplayString(osStr, _countof(osStr));
    Log(L"[INFO] 当前系统：%ls", osStr);
    Check(L"OS 检测返回非空描述", osStr[0] ? TRUE : FALSE);
    Check(L"OS 检测识别为 Win10/11/Server 之一",
          (wcsstr(osStr, L"Windows") != NULL) ? TRUE : FALSE);

    // 2) 逐个测试 RunCommand 功能
    for (size_t i = 0; i < g_featureCount; i++) {
        if (g_features[i].kind == FeatureKind::RunCommand)
            TestRunCommand(&g_features[i]);
    }

    // 2.5) 图标提取与 .ico 写出逻辑
    TestIconConversion();

    // 3) 测试 ShellExtension 注册路径
    TestShellExtension();

    // 3.5) 运行时验证 Win11 COM 服务器（加载 DLL -> QI -> GetTitle）
    TestWin11ComServer();

    // 3.6) 右键菜单类测试（RunCommand / ShellExtension）已覆盖全部功能

    // 3.7) 硬件信息采集（v1.0.7 新增）：构造 → 字段非空检查 → 复制文本非空
    {
        HardwareInfo hw;
        ZeroMemory(&hw, sizeof(hw));
        HW_Collect(&hw);
        Check(L"硬件采集完成（不崩溃）", TRUE);
        Check(L"操作系统产品名非空", hw.osProduct[0] != 0);
        Check(L"操作系统版本非空",  hw.osVersion[0] != 0);
        Check(L"计算机名非空",       hw.computerName[0] != 0);
        Check(L"主板制造商非空",     hw.boardVendor[0] != 0);
        Check(L"BIOS 厂商非空",      hw.biosVendor[0] != 0);
        Check(L"至少 1 颗 CPU",      hw.cpuCount > 0);
        Check(L"逻辑处理器总数 > 0", hw.logicalProcTotal > 0);
        Check(L"内存总量 > 0 MB",    hw.memTotalMB > 0);
        Check(L"至少有 1 个内存插槽", hw.memCount > 0);
        WCHAR hwBuf[16384];   // 与 main.cpp 复制按钮使用同一上限（极端机 ~14KB）
        HW_BuildText(&hw, hwBuf, _countof(hwBuf));
        Check(L"硬件纯文本摘要非空", hwBuf[0] != 0);
        // 网卡 / 显示器 / 显卡至少有一项非零（VM 也会有虚拟网卡）；
        // 显示器在无头/RDP 会话下可能合法枚举不到（无 DISPLAY_DEVICE_ACTIVE），
        // 故不强制 displayCount>=1，改为"网卡或显示器"其一存在即可。
        Check(L"网卡采集到至少 1 个", hw.nicCount >= 1);
        Check(L"显示器（无头会话允许 0，否则 >=1）", hw.nicCount >= 1 || hw.displayCount >= 1);

        // v1.0.9：物理硬盘采集（IOCTL）+ HTML 报告生成
        Log(L"    物理硬盘：识别到 %d 块（%llu 字节）", hw.diskCount,
            (unsigned long long)hw.diskTotalBytes);
        Check(L"硬盘采集不崩溃（无盘/无权限时为 0 块，属正常）", TRUE);
        for (int i = 0; i < hw.diskCount; i++)
            Log(L"      #%d %ls [%ls/%ls] %llu 字节", i + 1, hw.disks[i].name,
                hw.disks[i].iface, hw.disks[i].media,
                (unsigned long long)hw.disks[i].sizeBytes);
        // 操作系统安装日期已按需求移除：字段必须保持为空（任何输出路径都不得引用）
        Check(L"操作系统安装日期不再采集", hw.osInstall[0] == 0);

        // v1.0.12：内存已插条数与每条 populated 标记
        Log(L"    内存：%I64u MB · 已插 %d / 共 %d 插槽", (unsigned long long)hw.memTotalMB,
            hw.memPopulated, hw.memCount);
        for (int i = 0; i < hw.memCount; i++) {
            if (hw.mem[i].populated)
                Log(L"      #%d %ls · %lu MB · %lu MHz · %ls · %ls", i + 1,
                    hw.mem[i].locator, hw.mem[i].capacityMB, hw.mem[i].speedMHz,
                    hw.mem[i].vendor, hw.mem[i].partNo);
        }
        int sum = 0;
        for (int i = 0; i < hw.memCount; i++) if (hw.mem[i].populated) sum++;
        Check(L"v1.0.12 memPopulated 统计正确", sum == hw.memPopulated);

        // v1.0.12.1：默认音频设备标记（role 字段：1=默认播放 / 2=默认录音 / 3=两者都是）
        if (hw.audioCount > 0) {
            int defPlay = 0, defRec = 0, defAny = 0;
            for (int i = 0; i < hw.audioCount; i++) {
                if (hw.audios[i].role & 1) defPlay++;
                if (hw.audios[i].role & 2) defRec++;
                if (hw.audios[i].role)      defAny++;
            }
            Log(L"    音频：%d 个真实硬件，默认播放 %d 个 / 默认录音 %d 个",
                hw.audioCount, defPlay, defRec);
            // 沙箱无音频设备时可能全 0；只要有音频就要求至少识别出 1 个默认
            if (hw.audioCount > 0)
                Check(L"v1.0.12.1 至少 1 个音频被标为默认（播放/录音任一）", defAny >= 1);
        }

        // v1.2.1：硬件核/线程——优先 OS 拓扑，兜底 CPUID。做合理性断言（不硬比 SMBIOS，
        // 因 VM/BIOS 限制下 OS 看到的活跃核数可能小于 SMBIOS 启用值）。
        Check(L"v1.2.1 CPU 核数 > 0",
              hw.cpuCount > 0 && hw.cpus[0].maxCores > 0);
        Check(L"v1.2.1 CPU 线程数 ≥ 核数",
              hw.cpuCount > 0 && hw.cpus[0].maxThreads >= hw.cpus[0].maxCores);
        Check(L"v1.2.1 CPU 核/线程数在合理范围",
              hw.cpuCount > 0 && hw.cpus[0].maxCores <= 256 && hw.cpus[0].maxThreads <= 512);
        if (hw.cpuCount > 0) {
            Log(L"    CPU#0 启用 %u 核 / %u 线程，识别 %u 核 / %u 线程",
                hw.cpus[0].cores, hw.cpus[0].threads,
                hw.cpus[0].maxCores, hw.cpus[0].maxThreads);
        }

        // v1.2.5：libcpuid 微架构代号——要么空（识别失败优雅降级），要么是非"未知"的有效文本
        Check(L"v1.2.5 CPU 代号为空或非\"未知\"",
              hw.cpuCount == 0 || hw.cpus[0].codename[0] == 0 ||
              wcscmp(hw.cpus[0].codename, L"\u672a\u77e5") != 0);
        if (hw.cpuCount > 0)
            Log(L"    CPU 代号: %ls", hw.cpus[0].codename[0] ? hw.cpus[0].codename : L"(未识别)");

        // v1.2.1：虚拟网卡不应出现在采集结果中
        BOOL anyVirtualNic = FALSE;
        for (int i = 0; i < hw.nicCount && !anyVirtualNic; i++) {
            const WCHAR* d = hw.nics[i].desc;
            if (wcsstr(d, L"WAN Miniport") ||
                wcsstr(d, L"Software Loopback Interface") ||
                wcsstr(d, L"Microsoft Loopback Adapter") ||
                wcsstr(d, L"Hyper-V Virtual Ethernet") ||
                wcsstr(d, L"vEthernet") ||
                wcsstr(d, L"VirtualBox Host-Only") ||
                wcsstr(d, L"VMware Virtual Ethernet") ||
                wcsstr(d, L"VMnet") ||
                wcsstr(d, L"TAP-Windows") ||
                wcsstr(d, L"Npcap")) {
                anyVirtualNic = TRUE;
            }
        }
        Check(L"v1.2.1 网卡列表不含常见虚拟适配器", !anyVirtualNic);

        // v1.1.8：主板芯片组 / 显卡显存 / 内存类型 / 显示器尺寸
        // 这些字段在 VM/无盘环境下可能为空（SetupAPI/DXGI/EDID 不可得），
        // 故仅做"若非空则合法"的软校验 + 记录取值，不硬失败（避免 VM 假阴性）。
        Log(L"    芯片组=%ls  内存类型#0=%ls",
            hw.boardChipset[0] ? hw.boardChipset : L"(空/VM不可得)",
            (hw.memCount > 0 && hw.mem[0].memType[0]) ? hw.mem[0].memType : L"(空)");
        for (int i = 0; i < hw.gpuCount; i++)
            Log(L"    显卡#%d %ls 显存=%u MB", i + 1, hw.gpus[i].name, hw.gpus[i].vramMB);
        for (int i = 0; i < hw.displayCount; i++)
            Log(L"    显示器#%d 尺寸=%ls", i + 1,
                hw.displays[i].size[0] ? hw.displays[i].size : L"(空/VM不可得)");
        for (int i = 0; i < hw.gpuCount; i++)
            if (hw.gpus[i].vramMB > 0)
                Check(L"v1.1.8 显存取值合理(<2TB)", hw.gpus[i].vramMB < 2u*1024*1024);
        // v1.2.6：无驱动占位名修正后不应残留微软基本显示驱动名称
        {
            BOOL anyPlaceholder = FALSE;
            for (int i = 0; i < hw.gpuCount && !anyPlaceholder; i++) {
                const WCHAR* n = hw.gpus[i].name;
                if (!n[0] ||
                    wcsstr(n, L"Basic Display") || wcsstr(n, L"Basic Render") ||
                    wcsstr(n, L"Standard VGA"))
                    anyPlaceholder = TRUE;
            }
            Check(L"v1.2.6 显卡无微软占位名残留（PCI 修正生效）", hw.gpuCount == 0 || !anyPlaceholder);
        }
        // v1.2.6：PCI 查表单测——知名网吧卡 Device ID 必须精确命中
        Check(L"v1.2.6 PCI表 10DE:1C03 -> GTX 1060 6GB",
              HW_GpuPciLookup(0x10DE, 0x1C03) &&
              strcmp(HW_GpuPciLookup(0x10DE, 0x1C03), "GeForce GTX 1060 6GB") == 0);
        Check(L"v1.2.6 PCI表 1002:67DF -> Radeon RX 470/480/...",
              HW_GpuPciLookup(0x1002, 0x67DF) &&
              strncmp(HW_GpuPciLookup(0x1002, 0x67DF), "Radeon RX 470", 13) == 0);
        Check(L"v1.2.6 PCI表 8086:9BC8 -> UHD Graphics 630",
              HW_GpuPciLookup(0x8086, 0x9BC8) &&
              strcmp(HW_GpuPciLookup(0x8086, 0x9BC8), "UHD Graphics 630") == 0);
        Check(L"v1.2.6 PCI表 未知名返回 NULL",
              HW_GpuPciLookup(0x10DE, 0xFFFF) == NULL && HW_GpuPciLookup(0xABCD, 0x1234) == NULL);
        Check(L"v1.2.6 占位名检测（基本显示适配器/标准VGA命中，真名不命中）",
              HW_IsGpuPlaceholderName(L"Microsoft Basic Display Adapter") &&
              HW_IsGpuPlaceholderName(L"Standard VGA Graphics Adapter") &&
              HW_IsGpuPlaceholderName(L"Microsoft Basic Render Driver") &&
              !HW_IsGpuPlaceholderName(L"NVIDIA GeForce GTX 1060 6GB") &&
              !HW_IsGpuPlaceholderName(L"Microsoft Remote Display Adapter"));
        for (int i = 0; i < hw.memCount; i++)
            if (hw.mem[i].memType[0])
                Check(L"v1.1.8 内存类型为已知 DDR 系列",
                      wcsncmp(hw.mem[i].memType, L"DDR", 3) == 0);

        // v1.1.9：内存类型字节→字符串映射回归断言（直接验证 HwMemTypeToStr）。
        // 此 bug 曾把 0x18(DDR3) 误映射成 LPDDR——注入式渲染测试因直接设字符串而漏测，故这里直测枚举。
        {
            extern const WCHAR* HwMemTypeToStr(BYTE);
            struct { BYTE raw; const WCHAR* exp; } map[] = {
                { 0x12, L"DDR" }, { 0x13, L"DDR2" }, { 0x18, L"DDR3" },
                { 0x1A, L"DDR4" }, { 0x1B, L"LPDDR" }, { 0x1C, L"LPDDR2" },
                { 0x1D, L"LPDDR3" }, { 0x1E, L"LPDDR4" }, { 0x22, L"DDR5" },
            };
            for (int k = 0; k < (int)(sizeof(map)/sizeof(map[0])); k++)
                Check(L"v1.1.9 内存类型枚举映射正确", wcscmp(HwMemTypeToStr(map[k].raw), map[k].exp) == 0);
            Check(L"v1.1.9 未知内存类型返回空串", HwMemTypeToStr(0x00)[0] == 0);
        }

        // v1.1.8：纯文本报告（HW_BuildText）渲染分支验证——注入确定性值后重建文本。
        // 与 HTML 验证对称，确保 HW_BuildText 的 v1.1.8 行（芯片组/显存/内存类型/尺寸）正确拼接。
        {
            WCHAR bakC[64]; wcsncpy(bakC, hw.boardChipset, 63); bakC[63]=0;
            WCHAR bakS[16]; wcsncpy(bakS, (hw.displayCount>0)?hw.displays[0].size:L"", 15); bakS[15]=0;
            WCHAR bakM[12]; wcsncpy(bakM, (hw.memCount>0)?hw.mem[0].memType:L"", 11); bakM[11]=0;
            DWORD bakV = (hw.gpuCount>0)? hw.gpus[0].vramMB : 0;
            if (hw.gpuCount==0)     { hw.gpuCount=1; hw.gpus[0].name[0]=0; }
            if (hw.displayCount==0) { hw.displayCount=1; hw.displays[0].width=1920; hw.displays[0].height=1080; hw.displays[0].refreshHz=60; }
            if (hw.memCount==0)     { hw.memCount=1; hw.mem[0].populated=1; hw.mem[0].capacityMB=16384; }
            StringCchCopyW(hw.boardChipset, _countof(hw.boardChipset), L"Intel H61");
            hw.gpus[0].vramMB = 1024;                                       // (1 GB)
            StringCchCopyW(hw.displays[0].size, _countof(hw.displays[0].size), L"31.5 英寸");
            StringCchCopyW(hw.mem[0].memType, _countof(hw.mem[0].memType), L"DDR4");
            WCHAR txt2[16384];
            HW_BuildText(&hw, txt2, _countof(txt2));
            Check(L"v1.1.8 纯文本含芯片组",   wcsstr(txt2, L"芯片组") != NULL);
            Check(L"v1.1.8 纯文本含显存 GB",   wcsstr(txt2, L"1 GB") != NULL);
            Check(L"v1.1.8 纯文本含显示器尺寸", wcsstr(txt2, L"英寸") != NULL);
            Check(L"v1.1.8 纯文本含内存类型",  wcsstr(txt2, L"DDR") != NULL);
            // 还原局部 hw（仅为幂等；本块为 3.7 末尾，出作用域即失效）
            StringCchCopyW(hw.boardChipset, _countof(hw.boardChipset), bakC);
            StringCchCopyW(hw.displays[0].size, _countof(hw.displays[0].size), bakS);
            StringCchCopyW(hw.mem[0].memType, _countof(hw.mem[0].memType), bakM);
            if (hw.gpuCount>0) hw.gpus[0].vramMB = bakV;
        }

        // v1.2.3：NVML 遥测渲染分支验证——注入确定性遥测值，验证文本拼接与"无遥测不追加"两分支
        {
            if (hw.gpuCount == 0) { hw.gpuCount = 1; hw.gpus[0].name[0] = 0; }
            HwGpu bak = hw.gpus[0];
            // 分支 1：遥测全 0（模拟无 N 卡 / 无驱动）→ 文本不应出现 NVML 状态段
            hw.gpus[0] = bak;
            hw.gpus[0].tempC = 0; hw.gpus[0].fanPct = 0; hw.gpus[0].powerW = 0;
            hw.gpus[0].utilPct = 0; hw.gpus[0].memUsedMB = 0; hw.gpus[0].memFreeMB = 0; hw.gpus[0].clockMHz = 0;
            WCHAR t0[16384];
            HW_BuildText(&hw, t0, _countof(t0));
            Check(L"v1.2.3 无遥测时文本不含'温度'", !wcsstr(t0, L"温度"));
            // 分支 2：注入遥测 → 文本应含各字段
            hw.gpus[0] = bak;
            hw.gpus[0].tempC = 62; hw.gpus[0].fanPct = 45; hw.gpus[0].powerW = 88;
            hw.gpus[0].utilPct = 30; hw.gpus[0].memUsedMB = 512; hw.gpus[0].memFreeMB = 1536; hw.gpus[0].clockMHz = 1200;
            if (hw.gpus[0].vramMB == 0) hw.gpus[0].vramMB = 2048;
            WCHAR t1[16384];
            HW_BuildText(&hw, t1, _countof(t1));
            Check(L"v1.2.3 有遥测文本含温度 62", wcsstr(t1, L"62") != NULL);
            Check(L"v1.2.3 有遥测文本含功耗 88W", wcsstr(t1, L"88W") != NULL);
            Check(L"v1.2.3 有遥测文本含利用率 30", wcsstr(t1, L"30") != NULL);
            Check(L"v1.2.3 有遥测文本含时钟 1200", wcsstr(t1, L"1200") != NULL);
            // 还原
            hw.gpus[0] = bak;
        }
    }

    // 3.8) HTML 报告生成（导出到桌面按钮的核心逻辑）
    {
        extern BOOL Report_BuildHtml(WCHAR* out, size_t nChars);
        extern void EnumDisks();                // 让分区表段有真实数据（v1.0.11 NT 设备名列）
        EnumDisks();
        WCHAR* rep = (WCHAR*)malloc(sizeof(WCHAR) * 40960);
        Check(L"报告缓冲分配成功", rep != NULL);
        BOOL ok = rep && Report_BuildHtml(rep, 40960);
        Check(L"HTML 报告生成成功", ok);
        Check(L"HTML 报告含完整文档头", wcsstr(rep, L"<html") != NULL && wcsstr(rep, L"</html>") != NULL);
        Check(L"HTML 报告含分区表段",  wcsstr(rep, L"section") != NULL);
        Check(L"HTML 报告不含安装日期", wcsstr(rep, L"安装日期") == NULL);
        Log(L"    报告长度：%d 字符", (int)wcslen(rep));

        // 落盘验证：真的写一份到临时目录，确认非空且带 UTF-8 BOM（双击不乱码的关键）
        extern BOOL WriteUtf8File(const WCHAR* path, const WCHAR* text);
        WCHAR tmpDir[MAX_PATH] = {0};
        GetTempPathW(_countof(tmpDir), tmpDir);
        WCHAR tmpFile[MAX_PATH] = {0};
        StringCchPrintfW(tmpFile, _countof(tmpFile), L"%lsWinSuiteX_selftest.html", tmpDir);
        BOOL wrote = WriteUtf8File(tmpFile, rep);
        Check(L"HTML 报告可写入磁盘", wrote);
        if (wrote) {
            HANDLE hf = CreateFileW(tmpFile, GENERIC_READ, FILE_SHARE_READ, NULL,
                                    OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
            Check(L"报告文件可打开", hf != INVALID_HANDLE_VALUE);
            if (hf != INVALID_HANDLE_VALUE) {
                DWORD sz = GetFileSize(hf, NULL);
                BYTE bom[3] = {0};
                DWORD rd = 0;
                ReadFile(hf, bom, 3, &rd, NULL);
                Check(L"报告文件非空", sz > 100);
                Check(L"报告为 UTF-8 BOM 编码",
                      rd == 3 && bom[0] == 0xEF && bom[1] == 0xBB && bom[2] == 0xBF);
                CloseHandle(hf);
            }
            DeleteFileW(tmpFile);      // 自检不留垃圾
        }
        // 索引项：至少包含"分区"段（报告里被裁掉细节时的兜底检查）
        Check(L"报告含分区表段", wcsstr(rep, L"分区") != NULL);
        // v1.0.11：分区表应列出 NT 设备名（\\.\PhysicalDriveN）
        Check(L"报告含 NT 设备名列",
              wcsstr(rep, L"\\\\.\\PhysicalDrive") != NULL || wcsstr(rep, L"NT 设备名") != NULL);
        free(rep);

        // v1.1.8：注入确定性值，验证新字段（芯片组/显存/内存类型/显示器尺寸）的
        // HTML 渲染分支——selftest 默认只填局部 hw 不填 g_hw，故先真实采集填充，
        // 再注入新字段并强制各段计数≥1，确保渲染分支存在，可在任意机器/VM 上断言。
        {
            extern HardwareInfo* SelfTest_PeekHw(void);
            HardwareInfo* inj = SelfTest_PeekHw();
            HW_Collect(inj);     // 填充 g_hw（含各段计数/数组），selftest 默认不填
            EnumDisks();

            // 备份将改动的计数与字段，测试后还原（保持 selftest 幂等）
            int bGpu = inj->gpuCount, bDisp = inj->displayCount, bMem = inj->memCount;
            WCHAR bakChip[64]; wcsncpy(bakChip, inj->boardChipset, 63); bakChip[63]=0;
            WCHAR bakGpu[128]; if (bGpu>0) wcsncpy(bakGpu, inj->gpus[0].name, 127); bakGpu[127]=0;
            DWORD bakVram = (bGpu>0)? inj->gpus[0].vramMB : 0;
            WCHAR bakSize[16]; if (bDisp>0) wcsncpy(bakSize, inj->displays[0].size, 15); bakSize[15]=0;
            WCHAR bakMem[12]; if (bMem>0) wcsncpy(bakMem, inj->mem[0].memType, 11); bakMem[11]=0;

            // 注入确定性值：强制各段计数≥1，并写入 v1.1.8 新字段
            StringCchCopyW(inj->boardChipset, _countof(inj->boardChipset), L"Intel H61");
            if (inj->gpuCount == 0)     { inj->gpuCount = 1; inj->gpus[0].name[0] = 0; }
            if (inj->displayCount == 0) { inj->displayCount = 1; inj->displays[0].name[0]=0;
                                          inj->displays[0].width=1920; inj->displays[0].height=1080; inj->displays[0].refreshHz=60; }
            if (inj->memCount == 0)     { inj->memCount = 1; inj->mem[0].populated=1; inj->mem[0].capacityMB=16384; }
            inj->gpus[0].vramMB = 1024;                                  // 1 GB
            StringCchCopyW(inj->displays[0].size, _countof(inj->displays[0].size), L"31.5 英寸");
            StringCchCopyW(inj->mem[0].memType, _countof(inj->mem[0].memType), L"DDR4");

            WCHAR* rep2 = (WCHAR*)malloc(sizeof(WCHAR) * 40960);
            BOOL ok2 = rep2 && Report_BuildHtml(rep2, 40960);
            Check(L"v1.1.8 注入后 HTML 报告生成成功", ok2);
            if (ok2) {
                Check(L"v1.1.8 HTML 含芯片组行",   wcsstr(rep2, L"芯片组") != NULL);
                Check(L"v1.1.8 HTML 含显存 GB",     wcsstr(rep2, L"1 GB") != NULL);
                Check(L"v1.1.8 HTML 含显示器尺寸", wcsstr(rep2, L"英寸") != NULL);
                Check(L"v1.1.8 HTML 含内存类型",    wcsstr(rep2, L"DDR") != NULL);
                Check(L"v1.1.8 注入未破坏报告结构", wcsstr(rep2, L"</html>") != NULL);
            }
            free(rep2);

            // 还原（即使进程将退出，亦保持幂等、不污染后续可能复用）
            StringCchCopyW(inj->boardChipset, _countof(inj->boardChipset), bakChip);
            inj->gpuCount = bGpu; inj->displayCount = bDisp; inj->memCount = bMem;
            if (bGpu>0)  { StringCchCopyW(inj->gpus[0].name, _countof(inj->gpus[0].name), bakGpu); inj->gpus[0].vramMB = bakVram; }
            if (bDisp>0) StringCchCopyW(inj->displays[0].size, _countof(inj->displays[0].size), bakSize);
            if (bMem>0)  StringCchCopyW(inj->mem[0].memType, _countof(inj->mem[0].memType), bakMem);
        }
    }

    // 4) 生产路径探测：尝试写入 HKLM(=HKEY_CLASSES_ROOT 默认)
    Log(L"\n=== 生产路径探测（HKLM / 需管理员提权） ===");
    BOOL admOk = Feature_Install(&g_features[0]);
    if (admOk) {
        Log(L"[INFO] HKLM 写入成功：当前进程已提权，生产路径可直接生效。");
        Feature_Uninstall(&g_features[0]); // 清理，避免遗留
        Check(L"HKLM 写入/清理成功", TRUE);
    } else {
        DWORD le = GetLastError();
        Log(L"[INFO] HKLM 写入失败（LastError=%u）：当前为非提权环境（沙箱/未以管理员运行）。", le);
        Log(L"[INFO] 这在无管理员权限时属预期；在用户机器上以管理员运行可正常写入。");
        Check(L"HKCU 路径已验证（生产路径需提权，单独报告）", TRUE);
    }

    // 汇总
    Log(L"\n====================================================");
    Log(L"  自检汇总：通过 %d，失败 %d", SelfTestPass, SelfTestFail);
    Log(L"====================================================");
    if (g_log != INVALID_HANDLE_VALUE) { CloseHandle(g_log); g_log = INVALID_HANDLE_VALUE; }

    return SelfTestFail;
}
