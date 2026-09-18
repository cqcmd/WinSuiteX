#ifndef SELFTEST_H
#define SELFTEST_H

// 自检入口：在控制台/分配的控制台中打印详细结果，并写入 selftest_report.txt
// 返回失败项数（0 表示全部通过）
int SelfTestMain();

// 供管理器 UI 在完成自检后读取汇总
extern int SelfTestPass;
extern int SelfTestFail;

#endif // SELFTEST_H
