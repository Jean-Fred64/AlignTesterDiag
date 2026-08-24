//! High-Precision Floppy Disk Filesystem Payload Synthesizer (OS-Ready Format)
//!
//! Generates valid logical filesystem structures for floppy disks during low-level formatting:
//! - **IBM PC FAT12** (`Pc35Hd`, `Pc35Dd`, `Pc525Hd`, `Pc525Dd`, `Pc525DdOnHd`): Valid BPB boot sector,
//!   FAT12 allocation tables with media descriptor, and empty root directory.
//! - **Atari ST TOS FAT12** (`Atari35Dd`): TOS-compatible BPB with verified 16-bit word checksum (sum == 0x1234).
//! - **AmigaDOS OFS** (`Amiga35Dd`): `DOS\0` bootblock (Blocks 0 & 1) with calculated 32-bit checksum,
//!   `RootBlock` at Block 880 (checksummed), and `BitmapBlock` at Block 881 (checksummed).
//! - **Amstrad CPC 3.0" Data** (`Cpc30Data`): Standard CP/M catalogue initialization (0xE5 pattern).

use crate::hw::protocol::PresetProfile;

/// Filesystem initialization mode during formatting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FsInitMode {
    /// Raw unformatted sectors filled with standard byte `0xE5`
    #[default]
    Blank,
    /// OS-ready initialization with valid boot sector, file allocation structures, and root directory
    OsReady,
}

impl FsInitMode {
    /// Human-readable label for UI toggles and badges
    pub fn label(self) -> &'static str {
        match self {
            FsInitMode::Blank => "Blank (Raw 0xE5)",
            FsInitMode::OsReady => "OS-Ready (Boot & Root FS)",
        }
    }

    /// Short status label
    pub fn short_label(self) -> &'static str {
        match self {
            FsInitMode::Blank => "Blank",
            FsInitMode::OsReady => "OS-Ready",
        }
    }

    /// Toggles between Blank and OsReady modes
    pub fn toggle(self) -> Self {
        match self {
            FsInitMode::Blank => FsInitMode::OsReady,
            FsInitMode::OsReady => FsInitMode::Blank,
        }
    }

    /// Detailed description of the filesystem type generated for the given preset
    pub fn os_desc(preset: PresetProfile) -> &'static str {
        match preset {
            PresetProfile::Pc35Hd
            | PresetProfile::Pc35Dd
            | PresetProfile::Pc525Hd
            | PresetProfile::Pc525DdOnHd
            | PresetProfile::Pc525Dd => "DOS FAT12",
            PresetProfile::Atari35Dd => "Atari TOS",
            PresetProfile::Amiga35Dd => "AmigaDOS OFS",
            PresetProfile::Cpc30Data => "CP/M Data",
        }
    }
}

/// Generates the 512-byte raw sector payload for a specific cylinder, head, and sector ID.
pub fn generate_sector_payload(
    preset: PresetProfile,
    cyl: u8,
    head: u8,
    sector_id: u8,
    fs_mode: FsInitMode,
    out: &mut [u8; 512],
) {
    // Default raw fill: 0xE5
    out.fill(0xE5);

    if fs_mode == FsInitMode::Blank {
        return;
    }

    match preset {
        PresetProfile::Pc35Hd => generate_pc_dos_payload(preset, cyl, head, sector_id, out),
        PresetProfile::Pc35Dd => generate_pc_dos_payload(preset, cyl, head, sector_id, out),
        PresetProfile::Pc525Hd => generate_pc_dos_payload(preset, cyl, head, sector_id, out),
        PresetProfile::Pc525DdOnHd | PresetProfile::Pc525Dd => {
            generate_pc_dos_payload(preset, cyl, head, sector_id, out)
        }
        PresetProfile::Atari35Dd => generate_atari_tos_payload(cyl, head, sector_id, out),
        PresetProfile::Amiga35Dd => generate_amiga_payload(cyl, head, sector_id, out),
        PresetProfile::Cpc30Data => {
            // CP/M Data Catalogue is standard 0xE5
            out.fill(0xE5);
        }
    }
}

// ============================================================================
// IBM PC DOS FAT12 Generator
// ============================================================================

struct DosGeometry {
    bytes_per_sec: u16,
    secs_per_cluster: u8,
    reserved_secs: u16,
    num_fats: u8,
    root_entries: u16,
    total_secs: u16,
    media_desc: u8,
    secs_per_fat: u16,
    secs_per_track: u16,
    num_heads: u16,
}

impl DosGeometry {
    fn for_preset(preset: PresetProfile) -> Self {
        match preset {
            PresetProfile::Pc35Hd => Self {
                bytes_per_sec: 512,
                secs_per_cluster: 1,
                reserved_secs: 1,
                num_fats: 2,
                root_entries: 224,
                total_secs: 2880,
                media_desc: 0xF0,
                secs_per_fat: 9,
                secs_per_track: 18,
                num_heads: 2,
            },
            PresetProfile::Pc35Dd => Self {
                bytes_per_sec: 512,
                secs_per_cluster: 2,
                reserved_secs: 1,
                num_fats: 2,
                root_entries: 112,
                total_secs: 1440,
                media_desc: 0xF9,
                secs_per_fat: 3,
                secs_per_track: 9,
                num_heads: 2,
            },
            PresetProfile::Pc525Hd => Self {
                bytes_per_sec: 512,
                secs_per_cluster: 1,
                reserved_secs: 1,
                num_fats: 2,
                root_entries: 224,
                total_secs: 2400,
                media_desc: 0xF9,
                secs_per_fat: 7,
                secs_per_track: 15,
                num_heads: 2,
            },
            PresetProfile::Pc525DdOnHd | PresetProfile::Pc525Dd => Self {
                bytes_per_sec: 512,
                secs_per_cluster: 2,
                reserved_secs: 1,
                num_fats: 2,
                root_entries: 112,
                total_secs: 720,
                media_desc: 0xFD,
                secs_per_fat: 2,
                secs_per_track: 9,
                num_heads: 2,
            },
            _ => Self {
                bytes_per_sec: 512,
                secs_per_cluster: 1,
                reserved_secs: 1,
                num_fats: 2,
                root_entries: 224,
                total_secs: 2880,
                media_desc: 0xF0,
                secs_per_fat: 9,
                secs_per_track: 18,
                num_heads: 2,
            },
        }
    }

    fn root_dir_sectors(&self) -> u16 {
        (self.root_entries * 32).div_ceil(self.bytes_per_sec)
    }
}

fn chs_to_lba(cyl: u8, head: u8, sec_id: u8, secs_per_track: u16, num_heads: u16) -> u16 {
    let c = cyl as u16;
    let h = head as u16;
    let s = (sec_id.max(1) - 1) as u16;
    (c * num_heads + h) * secs_per_track + s
}

fn generate_pc_dos_payload(
    preset: PresetProfile,
    cyl: u8,
    head: u8,
    sector_id: u8,
    out: &mut [u8; 512],
) {
    let geom = DosGeometry::for_preset(preset);
    let lba = chs_to_lba(cyl, head, sector_id, geom.secs_per_track, geom.num_heads);

    let fat1_start = geom.reserved_secs;
    let fat1_end = fat1_start + geom.secs_per_fat;
    let fat2_start = fat1_end;
    let fat2_end = fat2_start + geom.secs_per_fat;
    let root_start = fat2_end;
    let root_end = root_start + geom.root_dir_sectors();

    if lba == 0 {
        // Sector 0: DOS Boot Sector (BPB)
        out.fill(0x00);
        // Jump instruction: EB 3C 90 (JMP SHORT 0x3C; NOP)
        out[0] = 0xEB;
        out[1] = 0x3C;
        out[2] = 0x90;
        // OEM Name: "MSDOS5.0"
        out[3..11].copy_from_slice(b"MSDOS5.0");
        // Bytes per sector
        out[11..13].copy_from_slice(&geom.bytes_per_sec.to_le_bytes());
        // Sectors per cluster
        out[13] = geom.secs_per_cluster;
        // Reserved sectors
        out[14..16].copy_from_slice(&geom.reserved_secs.to_le_bytes());
        // Number of FATs
        out[16] = geom.num_fats;
        // Root entries
        out[17..19].copy_from_slice(&geom.root_entries.to_le_bytes());
        // Total sectors (16-bit)
        out[19..21].copy_from_slice(&geom.total_secs.to_le_bytes());
        // Media Descriptor
        out[21] = geom.media_desc;
        // Sectors per FAT
        out[22..24].copy_from_slice(&geom.secs_per_fat.to_le_bytes());
        // Sectors per track
        out[24..26].copy_from_slice(&geom.secs_per_track.to_le_bytes());
        // Number of heads
        out[26..28].copy_from_slice(&geom.num_heads.to_le_bytes());
        // Hidden sectors (32-bit = 0)
        out[28..32].copy_from_slice(&0u32.to_le_bytes());
        // Total sectors 32-bit (0 when 16-bit total_secs != 0)
        out[32..36].copy_from_slice(&0u32.to_le_bytes());
        // Physical drive number (0x00 = Floppy A:)
        out[36] = 0x00;
        // Reserved / Current head (0x00)
        out[37] = 0x00;
        // Extended Boot Signature (0x29)
        out[38] = 0x29;
        // Volume Serial ID (e.g. 0x24111985)
        out[39..43].copy_from_slice(&0x24111985u32.to_le_bytes());
        // Volume Label: "NO NAME    " (11 bytes)
        out[43..54].copy_from_slice(b"NO NAME    ");
        // File System Type: "FAT12   " (8 bytes)
        out[54..62].copy_from_slice(b"FAT12   ");

        // Standard boot code message
        let msg = b"Non-system disk or disk error\r\nReplace and strike any key when ready\r\n";
        let offset = 62;
        out[offset..offset + msg.len()].copy_from_slice(msg);

        // Boot Sector Signature (0x55, 0xAA) at 510..512
        out[510] = 0x55;
        out[511] = 0xAA;
    } else if lba >= fat1_start && lba < fat1_end {
        // FAT 1 Table
        out.fill(0x00);
        if lba == fat1_start {
            // First sector of FAT1 contains Media Descriptor + 0xFF + 0xFF (Cluster 0 & 1 markers)
            out[0] = geom.media_desc;
            out[1] = 0xFF;
            out[2] = 0xFF;
        }
    } else if lba >= fat2_start && lba < fat2_end {
        // FAT 2 Table (Mirror of FAT 1)
        out.fill(0x00);
        if lba == fat2_start {
            out[0] = geom.media_desc;
            out[1] = 0xFF;
            out[2] = 0xFF;
        }
    } else if lba >= root_start && lba < root_end {
        // Root Directory (Clean / Empty entries = 0x00)
        out.fill(0x00);
    } else {
        // Data sectors: clean 0x00 for OS-Ready format
        out.fill(0x00);
    }
}

// ============================================================================
// Atari ST TOS FAT12 Generator
// ============================================================================

fn generate_atari_tos_payload(cyl: u8, head: u8, sector_id: u8, out: &mut [u8; 512]) {
    // Atari ST 720K standard geometry: 2 heads, 9 sectors/track, 80 tracks, 5 sectors/FAT, 112 root entries
    let secs_per_track = 9u16;
    let num_heads = 2u16;
    let lba = chs_to_lba(cyl, head, sector_id, secs_per_track, num_heads);

    let reserved_secs = 1u16;
    let secs_per_fat = 5u16;
    let fat1_start = reserved_secs;
    let fat1_end = fat1_start + secs_per_fat;
    let fat2_start = fat1_end;
    let fat2_end = fat2_start + secs_per_fat;
    let root_start = fat2_end;
    let _root_end = root_start + 7; // 112 * 32 / 512 = 7 sectors

    if lba == 0 {
        // Atari TOS Boot Sector
        out.fill(0x00);
        // Branch instruction: BRA.S +56 (0x60, 0x38) or NOPs (0x60, 0x1C)
        out[0] = 0x60;
        out[1] = 0x38;
        // OEM Name: 6 bytes "ALIGND"
        out[2..8].copy_from_slice(b"ALIGND");
        // 24-bit Serial Number (bytes 8..11)
        out[8] = 0x12;
        out[9] = 0x34;
        out[10] = 0x56;
        // Bytes per sector (512, little endian)
        out[11..13].copy_from_slice(&512u16.to_le_bytes());
        // Sectors per cluster (2)
        out[13] = 2;
        // Reserved sectors (1)
        out[14..16].copy_from_slice(&1u16.to_le_bytes());
        // Number of FATs (2)
        out[16] = 2;
        // Root entries (112)
        out[17..19].copy_from_slice(&112u16.to_le_bytes());
        // Total sectors (1440)
        out[19..21].copy_from_slice(&1440u16.to_le_bytes());
        // Media Descriptor (0xF9)
        out[21] = 0xF9;
        // Sectors per FAT (5)
        out[22..24].copy_from_slice(&5u16.to_le_bytes());
        // Sectors per track (9)
        out[24..26].copy_from_slice(&9u16.to_le_bytes());
        // Number of heads (2)
        out[26..28].copy_from_slice(&2u16.to_le_bytes());
        // Hidden sectors (0)
        out[28..30].copy_from_slice(&0u16.to_le_bytes());

        // Calculate Atari TOS 16-bit Boot Checksum such that the 16-bit sum of all 256 words equals 0x1234
        let mut sum = 0u16;
        for i in 0..255 {
            let offset = i * 2;
            let word = u16::from_be_bytes([out[offset], out[offset + 1]]);
            sum = sum.wrapping_add(word);
        }
        let chk_word = 0x1234u16.wrapping_sub(sum);
        let chk_bytes = chk_word.to_be_bytes();
        out[510] = chk_bytes[0];
        out[511] = chk_bytes[1];
    } else if lba >= fat1_start && lba < fat1_end {
        out.fill(0x00);
        if lba == fat1_start {
            out[0] = 0xF9;
            out[1] = 0xFF;
            out[2] = 0xFF;
        }
    } else if lba >= fat2_start && lba < fat2_end {
        out.fill(0x00);
        if lba == fat2_start {
            out[0] = 0xF9;
            out[1] = 0xFF;
            out[2] = 0xFF;
        }
    } else {
        out.fill(0x00);
    }
}

// ============================================================================
// AmigaDOS OFS Generator
// ============================================================================

/// Computes AmigaDOS 32-bit bootblock checksum over 256 longwords (1024 bytes = Block 0 & 1).
pub fn compute_amiga_boot_checksum(boot_blocks: &[u8; 1024]) -> u32 {
    let mut sum: u32 = 0;
    for i in 0..256 {
        if i == 1 {
            // Skip checksum longword at offset 4..8
            continue;
        }
        let offset = i * 4;
        let lw = u32::from_be_bytes([
            boot_blocks[offset],
            boot_blocks[offset + 1],
            boot_blocks[offset + 2],
            boot_blocks[offset + 3],
        ]);
        let (new_sum, carry) = sum.overflowing_add(lw);
        sum = new_sum.wrapping_add(if carry { 1 } else { 0 });
    }
    !sum // One's complement
}

/// Computes Amiga block checksum where the sum of all 128 longwords must be 0
pub fn compute_amiga_block_checksum(block: &[u8; 512], chk_offset_lw: usize) -> u32 {
    let mut sum: u32 = 0;
    for i in 0..128 {
        if i == chk_offset_lw {
            continue;
        }
        let offset = i * 4;
        let lw = u32::from_be_bytes([
            block[offset],
            block[offset + 1],
            block[offset + 2],
            block[offset + 3],
        ]);
        sum = sum.wrapping_add(lw);
    }
    0u32.wrapping_sub(sum)
}

fn generate_amiga_payload(cyl: u8, head: u8, sector_id: u8, out: &mut [u8; 512]) {
    // Amiga 880K DD geometry: 80 tracks, 2 heads, 11 sectors/track (sectors 0..10)
    let block_num = ((cyl as u32) * 2 + (head as u32)) * 11 + (sector_id as u32);

    match block_num {
        0 => {
            // Block 0: Boot block Part 1
            out.fill(0x00);
            // Header: "DOS\0" (OFS)
            out[0..4].copy_from_slice(b"DOS\0");
            // Rootblock pointer: 880 (0x00000370)
            out[8..12].copy_from_slice(&880u32.to_be_bytes());

            // Build temporary 1024-byte buffer (Block 0 + Block 1) to calculate Amiga bootblock checksum
            let mut two_blocks = [0u8; 1024];
            two_blocks[0..512].copy_from_slice(out);
            let chk = compute_amiga_boot_checksum(&two_blocks);
            out[4..8].copy_from_slice(&chk.to_be_bytes());
        }
        1 => {
            // Block 1: Boot block Part 2 (all 0s)
            out.fill(0x00);
        }
        880 => {
            // Block 880: AmigaDOS RootBlock (Cyl 40, Head 0, Sec 0)
            out.fill(0x00);
            // Longword 0: Primary Type T_HEADER = 2
            out[0..4].copy_from_slice(&2u32.to_be_bytes());
            // Longword 1: Header Key (own block 0)
            out[4..8].copy_from_slice(&0u32.to_be_bytes());
            // Longword 3: Hash Table Size = 72 (0x48)
            out[12..16].copy_from_slice(&72u32.to_be_bytes());
            // Longword 78 (offset 312): Bitmap Flag = 0xFFFFFFFF (Bitmap valid)
            out[312..316].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
            // Longword 79 (offset 316): Bitmap block pointer = 881 (0x00000371)
            out[316..320].copy_from_slice(&881u32.to_be_bytes());

            // Days since Jan 1 1978 (e.g. 17700), mins past midnight, ticks
            out[444..448].copy_from_slice(&17700u32.to_be_bytes());
            out[448..452].copy_from_slice(&720u32.to_be_bytes());
            out[452..456].copy_from_slice(&0u32.to_be_bytes());

            // Disk Name at offset 456: length (5) + "Empty"
            out[456] = 5;
            out[457..462].copy_from_slice(b"Empty");

            // Secondary Type ST_ROOT = 1 at offset 508..512
            out[508..512].copy_from_slice(&1u32.to_be_bytes());

            // Longword 5 (offset 20..24): RootBlock checksum (sum of all 128 longwords == 0)
            let chk = compute_amiga_block_checksum(out, 5);
            out[20..24].copy_from_slice(&chk.to_be_bytes());
        }
        881 => {
            // Block 881: AmigaDOS Bitmap Block (Cyl 40, Head 0, Sec 1)
            out.fill(0x00);
            // 1760 blocks on an 880K disk = 55 longwords of 32 bits
            // Bit 1 = free, Bit 0 = allocated
            // Allocated blocks: 0, 1 (boot), 880 (root), 881 (bitmap)
            // Longwords 1..55 start at offset 4
            for lw_idx in 1..=55 {
                let offset = lw_idx * 4;
                let val = if lw_idx == 1 {
                    // Blocks 0..31: bits 0 and 1 are allocated (0)
                    0xFFFF_FFFCu32
                } else if lw_idx == 28 {
                    // Blocks 864..895: block 880 is bit 16, block 881 is bit 17
                    // 0xFFFFFFFF ^ (1 << 16) ^ (1 << 17) = 0xFFFCFFFF
                    0xFFFC_FFFFu32
                } else {
                    0xFFFF_FFFFu32
                };
                out[offset..offset + 4].copy_from_slice(&val.to_be_bytes());
            }

            // Longword 0 (offset 0..4): Bitmap Block checksum
            let chk = compute_amiga_block_checksum(out, 0);
            out[0..4].copy_from_slice(&chk.to_be_bytes());
        }
        _ => {
            // All other data blocks on Amiga: clean 0x00
            out.fill(0x00);
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dos_bpb_and_fat12_generation_pc35hd() {
        let mut buf = [0u8; 512];
        generate_sector_payload(PresetProfile::Pc35Hd, 0, 0, 1, FsInitMode::OsReady, &mut buf);

        // Check Jump + OEM
        assert_eq!(&buf[0..3], &[0xEB, 0x3C, 0x90]);
        assert_eq!(&buf[3..11], b"MSDOS5.0");

        // Bytes per sector = 512
        assert_eq!(u16::from_le_bytes([buf[11], buf[12]]), 512);
        // Sectors per cluster = 1
        assert_eq!(buf[13], 1);
        // Reserved sectors = 1
        assert_eq!(u16::from_le_bytes([buf[14], buf[15]]), 1);
        // FATs = 2
        assert_eq!(buf[16], 2);
        // Root entries = 224
        assert_eq!(u16::from_le_bytes([buf[17], buf[18]]), 224);
        // Total sectors = 2880
        assert_eq!(u16::from_le_bytes([buf[19], buf[20]]), 2880);
        // Media descriptor = 0xF0
        assert_eq!(buf[21], 0xF0);
        // Sectors per FAT = 9
        assert_eq!(u16::from_le_bytes([buf[22], buf[23]]), 9);
        // Boot Signature = 0x55, 0xAA
        assert_eq!(buf[510], 0x55);
        assert_eq!(buf[511], 0xAA);

        // Check FAT1 start at Sector 2 (LBA 1)
        let mut fat_buf = [0u8; 512];
        generate_sector_payload(PresetProfile::Pc35Hd, 0, 0, 2, FsInitMode::OsReady, &mut fat_buf);
        assert_eq!(fat_buf[0], 0xF0);
        assert_eq!(fat_buf[1], 0xFF);
        assert_eq!(fat_buf[2], 0xFF);
        assert_eq!(fat_buf[3], 0x00);
    }

    #[test]
    fn test_atari_tos_bpb_and_checksum_verification() {
        let mut buf = [0u8; 512];
        generate_sector_payload(PresetProfile::Atari35Dd, 0, 0, 1, FsInitMode::OsReady, &mut buf);

        // Check Media Descriptor = 0xF9
        assert_eq!(buf[21], 0xF9);
        // Sectors per FAT = 5
        assert_eq!(u16::from_le_bytes([buf[22], buf[23]]), 5);

        // Verify that summing all 256 16-bit big-endian words equals 0x1234
        let mut sum = 0u16;
        for i in 0..256 {
            let offset = i * 2;
            let word = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
            sum = sum.wrapping_add(word);
        }
        assert_eq!(sum, 0x1234, "Atari ST TOS boot checksum must sum to 0x1234");
    }

    #[test]
    fn test_amiga_bootblock_checksum_verification() {
        let mut b0 = [0u8; 512];
        let mut b1 = [0u8; 512];
        generate_sector_payload(PresetProfile::Amiga35Dd, 0, 0, 0, FsInitMode::OsReady, &mut b0);
        generate_sector_payload(PresetProfile::Amiga35Dd, 0, 0, 1, FsInitMode::OsReady, &mut b1);

        assert_eq!(&b0[0..4], b"DOS\0");

        let mut two_blocks = [0u8; 1024];
        two_blocks[0..512].copy_from_slice(&b0);
        two_blocks[512..1024].copy_from_slice(&b1);

        // Verify Amiga boot checksum sum with end-around carry
        let mut sum: u32 = 0;
        for i in 0..256 {
            let offset = i * 4;
            let lw = u32::from_be_bytes([
                two_blocks[offset],
                two_blocks[offset + 1],
                two_blocks[offset + 2],
                two_blocks[offset + 3],
            ]);
            let (new_sum, carry) = sum.overflowing_add(lw);
            sum = new_sum.wrapping_add(if carry { 1 } else { 0 });
        }
        assert_eq!(sum, 0xFFFF_FFFF, "Amiga boot checksum must sum to 0xFFFFFFFF");
    }

    #[test]
    fn test_amiga_rootblock_and_bitmap_block_checksums() {
        let mut root_buf = [0u8; 512];
        // Block 880 = Cyl 40, Head 0, Sec 0
        generate_sector_payload(PresetProfile::Amiga35Dd, 40, 0, 0, FsInitMode::OsReady, &mut root_buf);

        // Verify RootBlock type and checksum
        let primary_type = u32::from_be_bytes([root_buf[0], root_buf[1], root_buf[2], root_buf[3]]);
        assert_eq!(primary_type, 2); // T_HEADER
        let sec_type = u32::from_be_bytes([root_buf[508], root_buf[509], root_buf[510], root_buf[511]]);
        assert_eq!(sec_type, 1); // ST_ROOT

        let mut root_sum = 0u32;
        for i in 0..128 {
            let offset = i * 4;
            let lw = u32::from_be_bytes([
                root_buf[offset],
                root_buf[offset + 1],
                root_buf[offset + 2],
                root_buf[offset + 3],
            ]);
            root_sum = root_sum.wrapping_add(lw);
        }
        assert_eq!(root_sum, 0, "Amiga RootBlock longwords sum must be 0");

        // Verify Bitmap Block 881 = Cyl 40, Head 0, Sec 1
        let mut bmp_buf = [0u8; 512];
        generate_sector_payload(PresetProfile::Amiga35Dd, 40, 0, 1, FsInitMode::OsReady, &mut bmp_buf);

        let mut bmp_sum = 0u32;
        for i in 0..128 {
            let offset = i * 4;
            let lw = u32::from_be_bytes([
                bmp_buf[offset],
                bmp_buf[offset + 1],
                bmp_buf[offset + 2],
                bmp_buf[offset + 3],
            ]);
            bmp_sum = bmp_sum.wrapping_add(lw);
        }
        assert_eq!(bmp_sum, 0, "Amiga BitmapBlock longwords sum must be 0");
    }

    #[test]
    fn test_blank_mode_returns_0xe5() {
        let mut buf = [0u8; 512];
        generate_sector_payload(PresetProfile::Pc35Hd, 0, 0, 1, FsInitMode::Blank, &mut buf);
        assert!(buf.iter().all(|&b| b == 0xE5));

        generate_sector_payload(PresetProfile::Amiga35Dd, 0, 0, 0, FsInitMode::Blank, &mut buf);
        assert!(buf.iter().all(|&b| b == 0xE5));
    }
}
