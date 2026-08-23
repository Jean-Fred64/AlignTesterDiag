//! High-Precision Low-Level Format Engine & MFM Track Synthesizer
//!
//! Provides zero-allocation MFM bitstream synthesis, pre-compensated flux translation (72 MHz),
//! CRC16-CCITT generation via lookup tables, Paula AmigaDOS split even/odd encoding,
//! and Greaseweazle `GW_FLUX` packet generation for low-level floppy formatting.

use crate::hw::protocol::PresetProfile;

/// Greaseweazle master sample clock frequency (72 MHz)
#[allow(dead_code)]
pub const GW_SAMPLE_FREQ_HZ: f64 = 72_000_000.0;

/// Write pre-compensation timing shift: 125 nanoseconds (9 ticks @ 72 MHz)
pub const WRITE_PRECOMP_TICKS: u32 = 9;

/// Inner track threshold for write pre-compensation (> Cylinder 40)
pub const WRITE_PRECOMP_MIN_CYL: u8 = 40;

// ============================================================================
// CRC16-CCITT Acceleration Table (Polynom 0x1021, Init 0xFFFF)
// ============================================================================

/// Compile-time generation of the 256-entry CRC16-CCITT lookup table (polynom 0x1021)
pub const fn generate_crc16_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut curr = (i as u16) << 8;
        let mut j = 0;
        while j < 8 {
            if (curr & 0x8000) != 0 {
                curr = (curr << 1) ^ 0x1021;
            } else {
                curr <<= 1;
            }
            j += 1;
        }
        table[i] = curr;
        i += 1;
    }
    table
}

/// Static CRC16-CCITT lookup table
pub const CRC16_TABLE: [u16; 256] = generate_crc16_table();

/// Accelerated CRC16-CCITT step using precalculated lookup table
#[inline(always)]
pub fn crc16_update(crc: u16, byte: u8) -> u16 {
    let idx = (((crc >> 8) as u8) ^ byte) as usize;
    (crc << 8) ^ CRC16_TABLE[idx]
}

/// Computes CRC16-CCITT over a slice with initial seed 0xFFFF
pub fn compute_crc16(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &b in data {
        crc = crc16_update(crc, b);
    }
    crc
}

// ============================================================================
// Static MFM Encoding Lookup Table
// ============================================================================

/// Compile-time generation of the 2x256 MFM encoding lookup table.
/// Table index 0: previous data bit was 0.
/// Table index 1: previous data bit was 1.
pub const fn generate_mfm_encode_table() -> [[u16; 256]; 2] {
    let mut table = [[0u16; 256]; 2];
    let mut prev_bit_idx = 0;
    while prev_bit_idx < 2 {
        let mut byte_val = 0;
        while byte_val < 256 {
            let mut mfm_word = 0u16;
            let mut prev = prev_bit_idx != 0;
            let mut bit_pos = 7;
            loop {
                let bit = ((byte_val >> bit_pos) & 1) != 0;
                let clock = !prev && !bit;
                let data = bit;
                let mfm_pair = ((if clock { 1 } else { 0 }) << 1) | (if data { 1 } else { 0 });
                mfm_word = (mfm_word << 2) | mfm_pair;
                prev = bit;
                if bit_pos == 0 {
                    break;
                }
                bit_pos -= 1;
            }
            table[prev_bit_idx][byte_val] = mfm_word;
            byte_val += 1;
        }
        prev_bit_idx += 1;
    }
    table
}

/// Static MFM byte-to-word encoding lookup table (2 x 256 entries)
pub const MFM_ENCODE_TABLE: [[u16; 256]; 2] = generate_mfm_encode_table();

/// Altered MFM sync word for 0xA1 with dropped clock transition: 0x4489 (binary `0100 0100 1000 1001`)
pub const MFM_SYNC_A1_DROPPED: u16 = 0x4489;

/// AmigaDOS sync word: 0x4489
pub const AMIGA_SYNC_WORD: u16 = 0x4489;

// ============================================================================
// Reusable Track & Flux Buffers (Zero-Allocation)
// ============================================================================

/// Reusable scratchpad buffers for synthesizing MFM tracks and Greaseweazle flux streams
pub struct MfmTrackBuffer {
    pub bits: Vec<bool>,
    pub flux_ticks: Vec<u32>,
    pub gw_flux_bytes: Vec<u8>,
}

impl Default for MfmTrackBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl MfmTrackBuffer {
    /// Creates a preallocated buffer capable of holding multiple revolutions of HD/DD flux
    pub fn new() -> Self {
        Self {
            bits: Vec::with_capacity(160_000),
            flux_ticks: Vec::with_capacity(80_000),
            gw_flux_bytes: Vec::with_capacity(131_072),
        }
    }

    /// Resets all buffers without deallocating their heap capacity
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.bits.clear();
        self.flux_ticks.clear();
        self.gw_flux_bytes.clear();
    }
}

// ============================================================================
// MFM Track Encoder
// ============================================================================

/// High-Precision MFM Track Encoder for standard retro diskette layouts
pub struct MfmTrackEncoder;

impl MfmTrackEncoder {
    /// Encodes a single byte into the output bit stream using the static MFM lookup table,
    /// updating `last_bit` in place.
    #[inline(always)]
    pub fn push_mfm_byte(bits: &mut Vec<bool>, byte: u8, last_bit: &mut bool) {
        let prev_idx = if *last_bit { 1 } else { 0 };
        let mfm_word = MFM_ENCODE_TABLE[prev_idx][byte as usize];
        for shift in (0..16).rev() {
            bits.push(((mfm_word >> shift) & 1) != 0);
        }
        *last_bit = (byte & 1) != 0;
    }

    /// Encodes multiple identical bytes with MFM formatting
    pub fn push_mfm_repeat(bits: &mut Vec<bool>, byte: u8, count: usize, last_bit: &mut bool) {
        for _ in 0..count {
            Self::push_mfm_byte(bits, byte, last_bit);
        }
    }

    /// Appends the altered sync word `0xA1*` (MFM `0x4489`) with dropped clock bit
    #[inline(always)]
    pub fn push_altered_sync_a1(bits: &mut Vec<bool>, last_bit: &mut bool) {
        for shift in (0..16).rev() {
            bits.push(((MFM_SYNC_A1_DROPPED >> shift) & 1) != 0);
        }
        *last_bit = true; // 0x4489 ends with data bit 1
    }

    /// Appends an Amiga sync word `0x4489`
    #[inline(always)]
    pub fn push_amiga_sync_word(bits: &mut Vec<bool>, last_bit: &mut bool) {
        for shift in (0..16).rev() {
            bits.push(((AMIGA_SYNC_WORD >> shift) & 1) != 0);
        }
        *last_bit = true;
    }

    /// Appends a raw 32-bit word directly to the bit stream (MSB first)
    pub fn push_u32_bits(bits: &mut Vec<bool>, val: u32) {
        for shift in (0..32).rev() {
            bits.push(((val >> shift) & 1) != 0);
        }
    }

    /// Synthesizes a full MFM track bitstream into the provided buffer based on the active PresetProfile
    pub fn encode_track_into(
        preset: PresetProfile,
        cyl: u8,
        head: u8,
        buffer: &mut MfmTrackBuffer,
    ) {
        buffer.bits.clear();
        let mut last_bit = false;

        match preset {
            PresetProfile::Amiga35Dd => {
                Self::encode_amiga_track(cyl, head, &mut buffer.bits, &mut last_bit);
            }
            PresetProfile::Pc35Hd => {
                // IBM PC 1.44M HD: 18 sectors, 512B, Gap1=80, Gap2=22, Gap3=84, Gap4b=approx 650, 500 kbps @ 300 RPM
                Self::encode_ibm_track(
                    cyl,
                    head,
                    18,
                    1,
                    80,
                    22,
                    84,
                    0xE5,
                    100_000,
                    &mut buffer.bits,
                    &mut last_bit,
                );
            }
            PresetProfile::Pc35Dd => {
                // IBM PC 720K DD: 9 sectors, 512B, Gap1=32, Gap2=22, Gap3=54, Gap4b=approx 600, 250 kbps @ 300 RPM
                Self::encode_ibm_track(
                    cyl,
                    head,
                    9,
                    1,
                    32,
                    22,
                    54,
                    0xE5,
                    50_000,
                    &mut buffer.bits,
                    &mut last_bit,
                );
            }
            PresetProfile::Pc525Hd => {
                // 5.25" HD (1.2M): 15 sectors, 512B, Gap1=80, Gap2=22, Gap3=84, 500 kbps @ 360 RPM (~83,333 bits)
                Self::encode_ibm_track(
                    cyl,
                    head,
                    15,
                    1,
                    80,
                    22,
                    84,
                    0xE5,
                    83_333,
                    &mut buffer.bits,
                    &mut last_bit,
                );
            }
            PresetProfile::Pc525DdOnHd => {
                // 5.25" DD on HD (360K @ 360 RPM): 9 sectors, 512B, Gap1=32, Gap2=22, Gap3=54, 300 kbps @ 360 RPM (~50,000 bits)
                Self::encode_ibm_track(
                    cyl,
                    head,
                    9,
                    1,
                    32,
                    22,
                    54,
                    0xE5,
                    50_000,
                    &mut buffer.bits,
                    &mut last_bit,
                );
            }
            PresetProfile::Pc525Dd => {
                // 5.25" DD (360K @ 300 RPM): 9 sectors, 512B, Gap1=32, Gap2=22, Gap3=54, 250 kbps @ 300 RPM (~50,000 bits)
                Self::encode_ibm_track(
                    cyl,
                    head,
                    9,
                    1,
                    32,
                    22,
                    54,
                    0xE5,
                    50_000,
                    &mut buffer.bits,
                    &mut last_bit,
                );
            }
            PresetProfile::Atari35Dd => {
                // Atari ST 720K: 9 sectors, 512B, Gap1=32, Gap2=22, Gap3=54, 250 kbps @ 300 RPM
                Self::encode_ibm_track(
                    cyl,
                    head,
                    9,
                    1,
                    32,
                    22,
                    54,
                    0xE5,
                    50_000,
                    &mut buffer.bits,
                    &mut last_bit,
                );
            }
            PresetProfile::Cpc30Data => {
                // Amstrad CPC 3.0" Data: 9 sectors (0xC1..0xC9), 512B, Gap1=32, Gap2=22, Gap3=54, 250 kbps @ 300 RPM
                Self::encode_ibm_track(
                    cyl,
                    head,
                    9,
                    0xC1,
                    32,
                    22,
                    54,
                    0xE5,
                    50_000,
                    &mut buffer.bits,
                    &mut last_bit,
                );
            }
        }
    }

    /// Internal generator for standard IBM PC / ISO track format (WD177x / uPD765 compliant)
    #[allow(clippy::too_many_arguments)]
    fn encode_ibm_track(
        cyl: u8,
        head: u8,
        sector_count: u8,
        first_sector_id: u8,
        gap1_len: usize,
        gap2_len: usize,
        gap3_len: usize,
        fill_byte: u8,
        nominal_rev_bits: usize,
        bits: &mut Vec<bool>,
        last_bit: &mut bool,
    ) {
        // 1. Post-Index Gap 1 (Lead-in Gap)
        Self::push_mfm_repeat(bits, 0x4E, gap1_len, last_bit);

        // 2. Sectors
        for sec_idx in 0..sector_count {
            let sec_id = first_sector_id + sec_idx;

            // --- ID Field ---
            // Sync: 12 bytes 0x00
            Self::push_mfm_repeat(bits, 0x00, 12, last_bit);

            // Altered Sync: 3x 0xA1* (0x4489)
            Self::push_altered_sync_a1(bits, last_bit);
            Self::push_altered_sync_a1(bits, last_bit);
            Self::push_altered_sync_a1(bits, last_bit);

            // IDAM (0xFE) + Cyl + Head + Sector + Size (2 = 512 bytes)
            let id_data = [0xA1, 0xA1, 0xA1, 0xFE, cyl, head, sec_id, 2];
            let id_crc = compute_crc16(&id_data);

            Self::push_mfm_byte(bits, 0xFE, last_bit);
            Self::push_mfm_byte(bits, cyl, last_bit);
            Self::push_mfm_byte(bits, head, last_bit);
            Self::push_mfm_byte(bits, sec_id, last_bit);
            Self::push_mfm_byte(bits, 2, last_bit);
            Self::push_mfm_byte(bits, (id_crc >> 8) as u8, last_bit);
            Self::push_mfm_byte(bits, (id_crc & 0xFF) as u8, last_bit);

            // Gap 2 (Inter-field Gap): 22 bytes 0x4E
            Self::push_mfm_repeat(bits, 0x4E, gap2_len, last_bit);

            // --- Data Field ---
            // Sync: 12 bytes 0x00
            Self::push_mfm_repeat(bits, 0x00, 12, last_bit);

            // Altered Sync: 3x 0xA1* (0x4489)
            Self::push_altered_sync_a1(bits, last_bit);
            Self::push_altered_sync_a1(bits, last_bit);
            Self::push_altered_sync_a1(bits, last_bit);

            // DAM (0xFB) + 512 bytes of data + CRC16
            let mut data_crc = 0xFFFFu16;
            data_crc = crc16_update(data_crc, 0xA1);
            data_crc = crc16_update(data_crc, 0xA1);
            data_crc = crc16_update(data_crc, 0xA1);
            data_crc = crc16_update(data_crc, 0xFB);

            Self::push_mfm_byte(bits, 0xFB, last_bit);
            for _ in 0..512 {
                data_crc = crc16_update(data_crc, fill_byte);
                Self::push_mfm_byte(bits, fill_byte, last_bit);
            }
            Self::push_mfm_byte(bits, (data_crc >> 8) as u8, last_bit);
            Self::push_mfm_byte(bits, (data_crc & 0xFF) as u8, last_bit);

            // Gap 3 (Inter-sector Gap)
            Self::push_mfm_repeat(bits, 0x4E, gap3_len, last_bit);
        }

        // 3. Gap 4b (Lead-out & Index Splice Area)
        // Add splice margin (~1.5% extra) to ensure complete revolution coverage
        let target_len = nominal_rev_bits + (nominal_rev_bits / 64);
        while bits.len() < target_len {
            Self::push_mfm_byte(bits, 0x4E, last_bit);
        }
    }

    /// Internal generator for AmigaDOS DD track format (11 sectors of 512B, Paula split even/odd MFM)
    fn encode_amiga_track(
        cyl: u8,
        head: u8,
        bits: &mut Vec<bool>,
        last_bit: &mut bool,
    ) {
        // Pre-track gap: 2 words of 0x0000
        Self::push_u32_bits(bits, 0xAAAA_AAAA);

        let track_num = (cyl << 1) | (head & 1);

        for sec_id in 0..11u8 {
            // 1. Sync: 2 words 0x44894489
            Self::push_amiga_sync_word(bits, last_bit);
            Self::push_amiga_sync_word(bits, last_bit);

            // 2. Info longword: format (0xFF) | track_num | sec_id | secs_to_gap (11 - sec_id)
            let format_byte = 0xFFu32;
            let secs_to_gap = (11 - sec_id) as u32;
            let info = (format_byte << 24)
                | ((track_num as u32) << 16)
                | ((sec_id as u32) << 8)
                | secs_to_gap;

            let even_info = (info >> 1) & 0x5555_5555;
            let odd_info = info & 0x5555_5555;
            Self::push_u32_bits(bits, even_info);
            Self::push_u32_bits(bits, odd_info);

            // 3. Sector Label (16 bytes = 4 longwords)
            let label = [0x00000000u32; 4];
            let mut even_labels = [0u32; 4];
            let mut odd_labels = [0u32; 4];
            for i in 0..4 {
                even_labels[i] = (label[i] >> 1) & 0x5555_5555;
                odd_labels[i] = label[i] & 0x5555_5555;
                Self::push_u32_bits(bits, even_labels[i]);
            }
            for i in 0..4 {
                Self::push_u32_bits(bits, odd_labels[i]);
            }

            // 4. Header Checksum: XOR over info + labels masked to 0x55555555
            let mut hdr_chk = even_info ^ odd_info;
            for i in 0..4 {
                hdr_chk ^= even_labels[i] ^ odd_labels[i];
            }
            hdr_chk &= 0x5555_5555;

            let even_hdr_chk = (hdr_chk >> 1) & 0x5555_5555;
            let odd_hdr_chk = hdr_chk & 0x5555_5555;
            Self::push_u32_bits(bits, even_hdr_chk);
            Self::push_u32_bits(bits, odd_hdr_chk);

            // 5. Data Field: 512 bytes = 128 longwords of 0xE5E5E5E5
            let mut data_lws = [0xE5E5E5E5u32; 128];
            // Optionally tag longword 0 with sector identifier
            data_lws[0] = 0xE5E50000 | (sec_id as u32);

            let mut even_data = [0u32; 128];
            let mut odd_data = [0u32; 128];
            let mut data_chk = 0u32;

            for i in 0..128 {
                even_data[i] = (data_lws[i] >> 1) & 0x5555_5555;
                odd_data[i] = data_lws[i] & 0x5555_5555;
                data_chk ^= even_data[i] ^ odd_data[i];
            }
            data_chk &= 0x5555_5555;

            let even_data_chk = (data_chk >> 1) & 0x5555_5555;
            let odd_data_chk = data_chk & 0x5555_5555;
            Self::push_u32_bits(bits, even_data_chk);
            Self::push_u32_bits(bits, odd_data_chk);

            for &d in &even_data {
                Self::push_u32_bits(bits, d);
            }
            for &d in &odd_data {
                Self::push_u32_bits(bits, d);
            }
        }

        // Post-track gap fill to nominal 50,000 bits + splice margin
        while bits.len() < 50_800 {
            Self::push_u32_bits(bits, 0xAAAA_AAAA);
        }
    }
}

// ============================================================================
// Flux Synthesizer (72 MHz Master Clock & Write Pre-compensation)
// ============================================================================

/// Translates MFM bit patterns into Greaseweazle 72 MHz sample intervals
/// with optional write pre-compensation and RLE byte emission.
pub struct FluxSynthesizer;

impl FluxSynthesizer {
    /// Computes the base clock tick length at 72 MHz for a given bitrate in kbps
    #[inline(always)]
    pub fn base_tick_clock(bitrate_kbps: u16) -> u32 {
        match bitrate_kbps {
            500 => 72,  // 1 µs = 72 ticks
            300 => 120, // 1.667 µs = 120 ticks
            250 => 144, // 2.0 µs = 144 ticks
            _ => 144,
        }
    }

    /// Synthesizes raw MFM bits into 72 MHz flux interval ticks with write pre-compensation
    pub fn bits_to_flux_ticks(
        bits: &[bool],
        bitrate_kbps: u16,
        cyl: u8,
        out_flux: &mut Vec<u32>,
    ) {
        out_flux.clear();
        let tick_unit = Self::base_tick_clock(bitrate_kbps);

        // 1. Extract raw transition interval counts (in bit cells)
        let mut count = 0u32;
        let mut first_transition = true;

        for &bit in bits {
            count += 1;
            if bit {
                if first_transition {
                    first_transition = false;
                } else {
                    out_flux.push(count * tick_unit);
                }
                count = 0;
            }
        }
        if count > 0 && !out_flux.is_empty() {
            out_flux.push(count * tick_unit);
        }

        // 2. Apply Write Pre-compensation if cylinder > 40
        if cyl > WRITE_PRECOMP_MIN_CYL && out_flux.len() >= 2 {
            let t2_nominal = 2 * tick_unit;
            let t3_nominal = 3 * tick_unit;
            let precomp = WRITE_PRECOMP_TICKS;

            let len = out_flux.len();
            for i in 0..len - 1 {
                let curr = out_flux[i];
                let next = out_flux[i + 1];

                let is_curr_2t = (curr as i32 - t2_nominal as i32).abs() <= 5;
                let is_next_2t = (next as i32 - t2_nominal as i32).abs() <= 5;
                let is_curr_wide = curr >= t3_nominal - 5;
                let is_next_wide = next >= t3_nominal - 5;

                if is_curr_2t && is_next_wide {
                    // Transition between curr and next is shifted EARLY (-precomp on curr, +precomp on next)
                    out_flux[i] = curr.saturating_sub(precomp);
                    out_flux[i + 1] = next.saturating_add(precomp);
                } else if is_curr_wide && is_next_2t {
                    // Transition between curr and next is shifted LATE (+precomp on curr, -precomp on next)
                    out_flux[i] = curr.saturating_add(precomp);
                    out_flux[i + 1] = next.saturating_sub(precomp);
                }
            }
        }
    }

    /// Encodes a list of 72 MHz flux interval ticks into the Greaseweazle v4 byte format
    pub fn flux_ticks_to_gw_bytes(flux_ticks: &[u32], out_bytes: &mut Vec<u8>) {
        out_bytes.clear();

        for &ticks in flux_ticks {
            if ticks == 0 {
                continue;
            }
            if ticks < 250 {
                out_bytes.push(ticks as u8);
            } else if ticks < 250 + 5 * 255 {
                let delta = ticks - 250;
                let high = 250 + (delta / 255) as u8;
                let low = ((delta % 255) + 1) as u8;
                out_bytes.push(high);
                out_bytes.push(low);
            } else {
                // Large intervals encoded as space packets: [0x00, 0x02, b0, b1, b2, b3]
                let val_28 = ticks & 0x0FFF_FFFF;
                let b0 = ((val_28 & 0x7F) << 1) as u8;
                let b1 = (((val_28 >> 7) & 0x7F) << 1) as u8;
                let b2 = (((val_28 >> 14) & 0x7F) << 1) as u8;
                let b3 = (((val_28 >> 21) & 0x7F) << 1) as u8;
                out_bytes.extend_from_slice(&[0x00, 0x02, b0, b1, b2, b3]);
            }
        }

        // FLUXOP_END terminator: [0x00, 0x00]
        out_bytes.push(0x00);
        out_bytes.push(0x00);
    }

    /// High-level single-pass synthesizer: transforms MFM bitstream into final Greaseweazle flux packet
    pub fn synthesize_track(
        preset: PresetProfile,
        cyl: u8,
        head: u8,
        buffer: &mut MfmTrackBuffer,
    ) {
        MfmTrackEncoder::encode_track_into(preset, cyl, head, buffer);
        Self::bits_to_flux_ticks(&buffer.bits, preset.target_data_rate(), cyl, &mut buffer.flux_ticks);
        Self::flux_ticks_to_gw_bytes(&buffer.flux_ticks, &mut buffer.gw_flux_bytes);
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hw::decode_amiga_sectors_from_bits;

    #[test]
    fn test_crc16_lookup_table_matches_reference() {
        let test_data = [
            vec![0x00u8; 10],
            vec![0xFFu8; 10],
            vec![0xA1, 0xA1, 0xA1, 0xFE, 0, 0, 1, 2],
            vec![0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0],
        ];

        for data in test_data {
            let mut crc_table = 0xFFFFu16;
            for &b in &data {
                crc_table = crc16_update(crc_table, b);
            }

            // Reference bit-by-bit calculation
            let mut crc_ref = 0xFFFFu16;
            for &b in &data {
                crc_ref ^= (b as u16) << 8;
                for _ in 0..8 {
                    if (crc_ref & 0x8000) != 0 {
                        crc_ref = (crc_ref << 1) ^ 0x1021;
                    } else {
                        crc_ref <<= 1;
                    }
                }
            }

            assert_eq!(crc_table, crc_ref, "CRC mismatch for {:?}", data);

            // Self-zeroing check: appending CRC MSB-first should result in CRC 0
            let mut full = data.clone();
            full.push((crc_table >> 8) as u8);
            full.push((crc_table & 0xFF) as u8);
            assert_eq!(compute_crc16(&full), 0, "CRC self-verification failed");
        }
    }

    #[test]
    fn test_mfm_encode_table_rule_verification() {
        // Bit 1 -> 01
        // Bit 0 after 0 -> 10
        // Bit 0 after 1 -> 00

        // 0x00 after 0 -> 10 10 10 10 10 10 10 10 = 0xAAAA
        assert_eq!(MFM_ENCODE_TABLE[0][0x00], 0xAAAA);
        // 0x00 after 1 -> 00 10 10 10 10 10 10 10 = 0x2AAA
        assert_eq!(MFM_ENCODE_TABLE[1][0x00], 0x2AAA);
        // 0xFF after 0 -> 01 01 01 01 01 01 01 01 = 0x5555
        assert_eq!(MFM_ENCODE_TABLE[0][0xFF], 0x5555);
        // 0xFF after 1 -> 01 01 01 01 01 01 01 01 = 0x5555
        assert_eq!(MFM_ENCODE_TABLE[1][0xFF], 0x5555);
    }

    #[test]
    fn test_altered_sync_a1_drop_clock_word() {
        assert_eq!(MFM_SYNC_A1_DROPPED, 0x4489);
    }

    #[test]
    fn test_flux_synthesizer_timings_250k_300k_500k() {
        assert_eq!(FluxSynthesizer::base_tick_clock(500), 72);
        assert_eq!(FluxSynthesizer::base_tick_clock(300), 120);
        assert_eq!(FluxSynthesizer::base_tick_clock(250), 144);
    }

    #[test]
    fn test_write_precompensation_outer_vs_inner_track() {
        // Pattern with 2T followed by 4T: 1010001
        let bits = vec![true, false, true, false, false, false, true];
        let mut flux_outer = Vec::new();
        let mut flux_inner = Vec::new();

        // Track 10 (no precomp)
        FluxSynthesizer::bits_to_flux_ticks(&bits, 500, 10, &mut flux_outer);
        // Track 60 (with precomp)
        FluxSynthesizer::bits_to_flux_ticks(&bits, 500, 60, &mut flux_inner);

        assert_eq!(flux_outer[0], 144); // 2T = 144 ticks
        assert_eq!(flux_outer[1], 288); // 4T = 288 ticks

        // With precomp: 2T followed by 4T is shifted early (-9 ticks on 2T, +9 ticks on 4T)
        assert_eq!(flux_inner[0], 144 - WRITE_PRECOMP_TICKS);
        assert_eq!(flux_inner[1], 288 + WRITE_PRECOMP_TICKS);
    }

    #[test]
    fn test_gw_flux_rle_roundtrip_decoding() {
        use crate::hw::decode_gw_flux;

        let sample_ticks = vec![72, 144, 216, 288, 360, 432, 576, 1200];
        let mut gw_bytes = Vec::new();
        FluxSynthesizer::flux_ticks_to_gw_bytes(&sample_ticks, &mut gw_bytes);

        let decoded = decode_gw_flux(&gw_bytes);
        assert_eq!(decoded, sample_ticks);
    }

    #[test]
    fn test_amiga_track_encoding_and_decoder_roundtrip() {
        let mut buffer = MfmTrackBuffer::new();
        MfmTrackEncoder::encode_track_into(PresetProfile::Amiga35Dd, 12, 1, &mut buffer);

        let sectors = decode_amiga_sectors_from_bits(&buffer.bits);
        assert_eq!(sectors.len(), 11, "Should decode exactly 11 Amiga sectors");

        for (i, sec) in sectors.iter().enumerate() {
            assert_eq!(sec.cyl, 12);
            assert_eq!(sec.head, 1);
            assert_eq!(sec.sec_id, i as u8);
            assert!(sec.crc_ok, "Amiga sector {} checksum should be valid", i);
        }
    }

    #[test]
    fn test_ibm_pc_hd_144m_track_synthesis_and_lengths() {
        let mut buffer = MfmTrackBuffer::new();
        FluxSynthesizer::synthesize_track(PresetProfile::Pc35Hd, 20, 0, &mut buffer);

        // 100,000 nominal bits + ~1.5% splice = >= 100,000 bits
        assert!(buffer.bits.len() >= 100_000, "Bitstream must be at least 100k bits, got {}", buffer.bits.len());
        assert!(!buffer.flux_ticks.is_empty());
        assert!(!buffer.gw_flux_bytes.is_empty());
        // Must terminate with [0x00, 0x00]
        assert_eq!(&buffer.gw_flux_bytes[buffer.gw_flux_bytes.len() - 2..], &[0x00, 0x00]);
    }

    #[test]
    fn test_ibm_pc_dd_720k_track_synthesis_and_lengths() {
        let mut buffer = MfmTrackBuffer::new();
        FluxSynthesizer::synthesize_track(PresetProfile::Pc35Dd, 40, 1, &mut buffer);

        assert!(buffer.bits.len() >= 50_000, "Bitstream must be at least 50k bits, got {}", buffer.bits.len());
        assert!(!buffer.flux_ticks.is_empty());
        assert_eq!(&buffer.gw_flux_bytes[buffer.gw_flux_bytes.len() - 2..], &[0x00, 0x00]);
    }

    #[test]
    fn test_pc_525_hd_12m_track_synthesis() {
        let mut buffer = MfmTrackBuffer::new();
        FluxSynthesizer::synthesize_track(PresetProfile::Pc525Hd, 10, 0, &mut buffer);

        assert!(buffer.bits.len() >= 83_333, "Bitstream must be at least 83.3k bits, got {}", buffer.bits.len());
        assert_eq!(&buffer.gw_flux_bytes[buffer.gw_flux_bytes.len() - 2..], &[0x00, 0x00]);
    }

    #[test]
    fn test_pc_525_dd_on_hd_300k_track_synthesis() {
        let mut buffer = MfmTrackBuffer::new();
        FluxSynthesizer::synthesize_track(PresetProfile::Pc525DdOnHd, 20, 1, &mut buffer);

        assert!(buffer.bits.len() >= 50_000, "Bitstream must be at least 50k bits, got {}", buffer.bits.len());
        assert_eq!(&buffer.gw_flux_bytes[buffer.gw_flux_bytes.len() - 2..], &[0x00, 0x00]);
    }

    #[test]
    fn test_atari_and_cpc_track_synthesis() {
        let mut buffer = MfmTrackBuffer::new();
        FluxSynthesizer::synthesize_track(PresetProfile::Atari35Dd, 0, 0, &mut buffer);
        assert!(buffer.bits.len() >= 50_000);

        FluxSynthesizer::synthesize_track(PresetProfile::Cpc30Data, 0, 0, &mut buffer);
        assert!(buffer.bits.len() >= 50_000);
    }

    #[test]
    fn test_format_progress_metrics_structure_and_formula() {
        use crate::hw::protocol::{FormatProgress, FormatStep};

        // 1. Single Head Single Track Format (1 pass)
        let prog_single = FormatProgress {
            current_track: 12,
            current_head: 0,
            total_tracks: 1,
            total_heads: 1,
            step: FormatStep::Completed,
            completed_passes: 1,
            total_passes: 1,
            verification_ok: true,
            retry_count: 0,
            quality_pct: 100,
            crc_errors: 0,
            verified_sectors: 18,
            expected_sectors: 18,
            elapsed_secs: 0.42,
            eta_secs: 0.0,
            message: "Track Format Verified OK".to_string(),
        };
        let pct_single = (prog_single.completed_passes as f32 / prog_single.total_passes as f32) * 100.0;
        assert_eq!(pct_single, 100.0);

        // 2. Full 40-Track Disk Format (80 passes) at 25% (20 passes) and 50% (40 passes)
        let mut prog_40 = FormatProgress {
            current_track: 10,
            current_head: 0,
            total_tracks: 40,
            total_heads: 2,
            step: FormatStep::Writing,
            completed_passes: 20,
            total_passes: 80,
            verification_ok: false,
            retry_count: 0,
            quality_pct: 100,
            crc_errors: 0,
            verified_sectors: 9,
            expected_sectors: 9,
            elapsed_secs: 8.4,
            eta_secs: 25.2,
            message: "Formatting...".to_string(),
        };
        assert_eq!((prog_40.completed_passes as f32 / prog_40.total_passes as f32) * 100.0, 25.0);

        prog_40.completed_passes = 40;
        assert_eq!((prog_40.completed_passes as f32 / prog_40.total_passes as f32) * 100.0, 50.0);

        prog_40.completed_passes = 80;
        prog_40.step = FormatStep::Completed;
        assert_eq!((prog_40.completed_passes as f32 / prog_40.total_passes as f32) * 100.0, 100.0);
    }
}
