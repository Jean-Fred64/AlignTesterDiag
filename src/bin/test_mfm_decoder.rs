#[allow(dead_code)]
fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[allow(dead_code)]
fn mfm_word_to_byte(bits: &[bool]) -> u8 {
    let mut b = 0u8;
    for i in 0..8 {
        if bits[i * 2 + 1] {
            b |= 1 << (7 - i);
        }
    }
    b
}

#[allow(dead_code)]
pub fn pll_flux_to_mfm_bits(flux: &[u8], clock_centre: f64) -> Vec<bool> {
    let mut bit_array = Vec::with_capacity(flux.len() * 4);
    let mut ticks = 0.0f64;
    let mut clock = clock_centre;
    let clock_min = clock_centre * 0.85;
    let clock_max = clock_centre * 1.15;
    let pll_phase_adj = 0.05f64;
    let pll_period_adj = 0.05f64;

    let mut space_ticks = 0u64;
    for &b in flux {
        if (1..=249).contains(&b) {
            let x = (space_ticks + (b as u64)) as f64;
            space_ticks = 0;

            ticks += x;
            if ticks < clock / 2.0 {
                continue;
            }

            let mut zeros = 0;
            while ticks >= clock / 2.0 {
                ticks -= clock;
                if ticks < clock / 2.0 {
                    break;
                }
                zeros += 1;
                bit_array.push(false);
            }
            bit_array.push(true);

            let new_ticks = ticks * (1.0 - pll_phase_adj);
            if zeros <= 3 {
                clock += ticks * pll_period_adj;
            } else {
                clock += (clock_centre - clock) * pll_period_adj;
            }
            clock = clock.clamp(clock_min, clock_max);
            ticks = new_ticks;
        } else if b == 0xFA {
            space_ticks += 250;
        } else {
            space_ticks = 0;
        }
    }
    bit_array
}

#[allow(dead_code)]
fn decode_idam_sectors(raw_mfm_bits: &[bool]) -> Vec<(u8, u8, u8, u8, bool)> {
    let mut sectors = Vec::new();
    let sync_word: u16 = 0x4489;
    let mut shift_reg: u16 = 0;

    let mut i = 0;
    while i + 160 <= raw_mfm_bits.len() {
        let bit = raw_mfm_bits[i];
        shift_reg = (shift_reg << 1) | (if bit { 1 } else { 0 });

        if shift_reg == sync_word {
            let mut offset = i + 1;
            while offset + 16 <= raw_mfm_bits.len() {
                let mut w: u16 = 0;
                for k in 0..16 {
                    w = (w << 1) | (if raw_mfm_bits[offset + k] { 1 } else { 0 });
                }
                if w == 0x4489 {
                    offset += 16;
                } else {
                    break;
                }
            }

            if offset + 7 * 16 <= raw_mfm_bits.len() {
                let mut header_bytes = Vec::with_capacity(7);
                for byte_idx in 0..7 {
                    let b = mfm_word_to_byte(
                        &raw_mfm_bits[offset + byte_idx * 16..offset + (byte_idx + 1) * 16],
                    );
                    header_bytes.push(b);
                }

                if header_bytes[0] == 0xFE {
                    let cyl = header_bytes[1];
                    let head = header_bytes[2];
                    let sec_id = header_bytes[3];
                    let size_code = header_bytes[4];

                    if (1..=20).contains(&sec_id) && size_code <= 4 {
                        let mut crc_buf = vec![0xA1, 0xA1, 0xA1];
                        crc_buf.extend_from_slice(&header_bytes[0..5]);
                        let calc_crc = crc16_ccitt(&crc_buf);
                        let read_crc = ((header_bytes[5] as u16) << 8) | (header_bytes[6] as u16);

                        if !sectors
                            .iter()
                            .any(|s: &(u8, u8, u8, u8, bool)| s.0 == cyl && s.2 == sec_id)
                        {
                            sectors.push((cyl, head, sec_id, size_code, read_crc == calc_crc));
                        }

                        i = offset + 7 * 16;
                        shift_reg = 0;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    sectors
}

fn main() {
    println!("PLL test binary compiled successfully.");
}
