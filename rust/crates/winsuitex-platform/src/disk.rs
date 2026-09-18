//! 磁盘/分区信息源：盘符、簇大小、文件系统、容量、卷标。
//!
//! 说明：`disk_index`（盘符 → PhysicalDriveN 映射）需要
//! `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS`，本期暂未实现（保持 `None`）。

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

use crate::{to_wide, wide_to_string};
use winsuitex_core::error::Result;
use winsuitex_core::model::Volume;
use winsuitex_core::traits::{CollectContext, HardwareSource};

fn drive_type_name(t: u32) -> Option<&'static str> {
    Some(match t {
        2 => "可移动磁盘",
        3 => "本地磁盘",
        4 => "网络磁盘",
        5 => "光盘驱动器",
        6 => "RAM 磁盘",
        _ => "未知",
    })
}

pub struct DiskSource;

impl HardwareSource for DiskSource {
    fn name(&self) -> &'static str {
        "disk"
    }

    fn collect(&self, ctx: &mut CollectContext) -> Result<()> {
        // SAFETY: GetLogicalDrives 无参数，返回盘符位掩码。
        let mask = unsafe { GetLogicalDrives() };
        let mut volumes = Vec::new();

        for i in 0..26u8 {
            if mask & (1u32 << i) == 0 {
                continue;
            }
            let letter = char::from(b'A' + i);
            let root = format!("{}:\\", letter);
            let wide = to_wide(&root);

            let (mut spc, mut bps, mut free_clusters, mut total_clusters) =
                (0u32, 0u32, 0u32, 0u32);
            // SAFETY: wide 在本轮循环内有效；四个 out 参数均为栈上 u32。
            let space_ok = unsafe {
                GetDiskFreeSpaceW(
                    PCWSTR(wide.as_ptr()),
                    Some(&mut spc),
                    Some(&mut bps),
                    Some(&mut free_clusters),
                    Some(&mut total_clusters),
                )
            }
            .is_ok();

            // SAFETY: wide 有效。
            let dtype = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };

            let mut vol_buf = vec![0u16; 256];
            let mut fs_buf = vec![0u16; 256];
            // SAFETY: 两个缓冲区容量均为 256 个 u16，API 不会越界写入。
            let vi_ok = unsafe {
                GetVolumeInformationW(
                    PCWSTR(wide.as_ptr()),
                    Some(&mut vol_buf[..]),
                    None,
                    None,
                    None,
                    Some(&mut fs_buf[..]),
                )
            }
            .is_ok();

            // 簇字节数用 checked_mul：C++ 版直接相乘有溢出风险
            let cluster_bytes = spc.checked_mul(bps);
            let bytes_per_cluster = cluster_bytes.map(u64::from);

            volumes.push(Volume {
                letter: format!("{}:", letter),
                type_name: drive_type_name(dtype).map(str::to_string),
                fs_name: if vi_ok { wide_to_string(&fs_buf) } else { None },
                sectors_per_cluster: space_ok.then_some(spc),
                bytes_per_sector: space_ok.then_some(bps),
                cluster_bytes: if space_ok { cluster_bytes } else { None },
                total_bytes: space_ok
                    .then(|| bytes_per_cluster.map(|b| b * u64::from(total_clusters)))
                    .flatten(),
                free_bytes: space_ok
                    .then(|| bytes_per_cluster.map(|b| b * u64::from(free_clusters)))
                    .flatten(),
                vol_label: if vi_ok {
                    wide_to_string(&vol_buf)
                } else {
                    None
                },
                disk_index: None,
            });
        }

        ctx.set_volumes(volumes);
        Ok(())
    }
}
