fn get_sector_order(sector_count: u8) -> Vec<u8> {
    match sector_count {
        9 => vec![1, 6, 2, 7, 3, 8, 4, 9, 5],
        15 => vec![1, 9, 2, 10, 3, 11, 4, 12, 5, 13, 6, 14, 7, 15, 8],
        _ => vec![
            3, 13, 7, 1, 12, 6, 10, 5, 15, 9, 8, 2, 11, 4, 14, 16, 17, 18,
        ],
    }
}

fn main() {
    println!("=== SECTOR GENERATION TEST BY FORMAT ===");

    for &cnt in &[9, 15, 18] {
        let order = get_sector_order(cnt);
        let max_sec = order.iter().cloned().max().unwrap_or(0);
        let min_sec = order.iter().cloned().min().unwrap_or(0);

        println!("\nFormat {} sectors / track :", cnt);
        println!("  -> Interleave order : {:?}", order);
        println!("  -> Sector range : Sector {} to {}", min_sec, max_sec);

        if cnt == 9 {
            assert!(max_sec <= 9, "ERROR: Sector > 9 found in DD mode!");
            println!("  [OK] Validation 720K DD (Sectors 1 to 9 only)");
        } else if cnt == 15 {
            assert!(max_sec <= 15, "ERROR: Sector > 15 found in 15-sec mode!");
            println!("  [OK] Validation 1.2M HD (Sectors 1 to 15 only)");
        } else if cnt == 18 {
            assert!(max_sec <= 18, "ERROR: Sector > 18 found in 18-sec mode!");
            println!("  [OK] Validation 1.44M HD (Sectors 1 to 18)");
        }
    }
}
