fn main() {
    // центр сетки Новосибирска, 6 ноября 2026 (310-й день года)
    let d = declination::declination_deg(54.9467, 83.0803, 2026, 310).unwrap();
    let total = (d * 60.0).round() as i32;
    println!("{d:.4}° = {}°{}′", total / 60, total % 60);
}
