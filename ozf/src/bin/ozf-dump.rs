use ozf::Ozf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("использование: ozf-dump файл.ozf2 [номер_уровня выходной.png]");
        std::process::exit(2);
    }
    let mut o = match Ozf::open(&args[1]) {
        Ok(o) => o,
        Err(e) => { eprintln!("ошибка: {e}"); std::process::exit(1); }
    };
    for (i, l) in o.levels.iter().enumerate() {
        println!("уровень {i}: {}x{} px, тайлов {}x{}", l.width, l.height, l.tiles_x, l.tiles_y);
    }
    if args.len() >= 4 {
        let lv: usize = args[2].parse().expect("номер уровня");
        match o.level_rgb(lv) {
            Ok((w, h, img)) => {
                image::save_buffer(&args[3], &img, w, h, image::ColorType::Rgb8)
                    .expect("запись PNG");
                println!("записано {} ({w}x{h})", args[3]);
            }
            Err(e) => { eprintln!("ошибка: {e}"); std::process::exit(1); }
        }
    }
}
