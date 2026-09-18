//! 排障装饰器：为每个采集源加上 **堆完整性校验**。
//!
//! # 为什么需要它
//!
//! 真机（网吧环境）上出现过 `0xC0000374 STATUS_HEAP_CORRUPTION`：进程被 Windows
//! 堆管理器直接终止，没有 panic 消息、没有错误输出，stdout 缓冲区内容全部丢失，
//! 只剩一个 0 字节的重定向文件。
//!
//! 堆损坏有个阴险特性：**被发现的时刻 ≠ 被写坏的时刻**。某个源写越界踩坏了堆，
//! 往往要到后面某个源分配 / 释放内存时才触发检测。因此只知道"崩在 audio"并不等于
//! "audio 是凶手"——必须能区分这两种情况：
//!
//! - `before X = CORRUPT` → X 之前就坏了，凶手在更早的源里；
//! - `before X = ok` 且 `after X = CORRUPT` → X 就是凶手（或它的某次调用）。
//!
//! 实现方式：用 `HeapValidate(GetProcessHeap(), 0, NULL)` 校验进程默认堆的全部内存块。
//! 该调用只在 `WSX_TRACE=1` 时执行，正常路径零开销（一次 bool 判断）。

use windows::Win32::System::Memory::{GetProcessHeap, HeapValidate, HEAP_FLAGS};

use winsuitex_core::error::Result;
use winsuitex_core::traits::{
    wsx_trace_enabled_pub, CollectContext, HardwareSource, ThreadAffinity,
};

/// 校验进程默认堆的完整性。
///
/// `HeapValidate` 遍历堆内所有块，`None` 表示校验整个堆（而非单个块）。
/// SAFETY: `GetProcessHeap` 返回的 HANDLE 对本进程始终有效；`HeapValidate` 是只读校验，
/// 不修改任何内存，失败时只返回 FALSE，不会影响进程状态。
fn heap_ok() -> bool {
    let Ok(heap) = (unsafe { GetProcessHeap() }) else {
        return false;
    };
    unsafe { HeapValidate(heap, HEAP_FLAGS(0), None) }.as_bool()
}

/// 把任意 `HardwareSource` 包一层：trace 开启时打印进出打点与堆状态。
pub struct TracedSource<S> {
    inner: S,
}

impl<S> TracedSource<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: HardwareSource> HardwareSource for TracedSource<S> {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        // 正常路径零开销：不开 trace 时直接透传，连一次堆校验都不做。
        if !wsx_trace_enabled_pub() {
            return self.inner.collect(ctx);
        }

        let n = self.inner.name();
        let before = heap_ok();
        eprintln!(
            "[heap] before {:<16} {}",
            n,
            if before { "ok" } else { "CORRUPT" }
        );

        let r = self.inner.collect(ctx);

        let after = heap_ok();
        eprintln!(
            "[heap] after  {:<16} {}",
            n,
            if after { "ok" } else { "CORRUPT" }
        );
        if before && !after {
            eprintln!(
                "[heap] >>> in <<< heap corrupted during '{}' - this source is the culprit",
                n
            );
        }
        r
    }

    fn affinity(&self) -> ThreadAffinity {
        self.inner.affinity()
    }
}
