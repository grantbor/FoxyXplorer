//! Чтение растровых карт OziExplorer .ozf2 (только чтение).
//! Формат выведен по реальным файлам; OZFx3 (шифрованный) не поддерживается.

use flate2::read::ZlibDecoder;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const TILE: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum OzfError {
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),
    #[error("не OZF2: неверная сигнатура {0:#06x}")]
    BadSignature(u16),
    #[error("повреждённая структура файла: {0}")]
    Corrupt(String),
    #[error("уровень {0} отсутствует")]
    NoLevel(usize),
    #[error("тайл ({0}, {1}) вне уровня")]
    NoTile(u32, u32),
}

type Result<T> = std::result::Result<T, OzfError>;

#[derive(Debug, Clone)]
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub tiles_x: u32,
    pub tiles_y: u32,
    /// Палитра: 256 цветов в порядке RGB.
    pub palette: Vec<[u8; 3]>,
    tile_offsets: Vec<u32>,
}

pub struct Ozf {
    file: File,
    pub levels: Vec<Level>,
}

fn u16_at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([b[p], b[p + 1]])
}
fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}

impl Ozf {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut file = File::open(path)?;
        let size = file.seek(SeekFrom::End(0))?;
        if size < 0x60 {
            return Err(OzfError::Corrupt("файл слишком мал".into()));
        }

        let mut head = [0u8; 0x40];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut head)?;
        let sig = u16_at(&head, 0);
        if sig != 0x7778 {
            return Err(OzfError::BadSignature(sig));
        }
        let nlev = u16_at(&head, 0x3a) as usize;

        // Указатель на таблицу заголовков уровней — последние 4 байта.
        let mut tail = [0u8; 4];
        file.seek(SeekFrom::Start(size - 4))?;
        file.read_exact(&mut tail)?;
        let table = u32::from_le_bytes(tail) as u64;
        if table >= size - 4 || (size - 4 - table) % 4 != 0 {
            return Err(OzfError::Corrupt("неверный указатель таблицы уровней".into()));
        }
        let n = ((size - 4 - table) / 4) as usize;
        if n != nlev + 2 {
            return Err(OzfError::Corrupt(format!(
                "записей в таблице {n}, ожидалось {}",
                nlev + 2
            )));
        }

        let mut raw = vec![0u8; n * 4];
        file.seek(SeekFrom::Start(table))?;
        file.read_exact(&mut raw)?;
        let offsets: Vec<u32> = (0..n).map(|i| u32_at(&raw, i * 4)).collect();

        let mut levels = Vec::with_capacity(n);
        for (i, off) in offsets.iter().enumerate() {
            levels.push(Self::read_level(&mut file, *off as u64, size).map_err(|e| {
                OzfError::Corrupt(format!("уровень {i}: {e}"))
            })?);
        }
        Ok(Ozf { file, levels })
    }

    fn read_level(file: &mut File, off: u64, size: u64) -> Result<Level> {
        let mut h = [0u8; 12];
        file.seek(SeekFrom::Start(off))?;
        file.read_exact(&mut h)?;
        let width = u32_at(&h, 0);
        let height = u32_at(&h, 4);
        let tiles_x = u16_at(&h, 8) as u32;
        let tiles_y = u16_at(&h, 10) as u32;
        if width == 0 || height == 0 || tiles_x == 0 || tiles_y == 0 {
            return Err(OzfError::Corrupt("нулевой размер".into()));
        }
        // Файл хранит floor(size/64)+1 тайлов по каждой оси: при размере,
        // кратном 64, добавляется лишний пустой тайл. Допускаем оба варианта.
        let min_x = width.div_ceil(TILE as u32);
        let min_y = height.div_ceil(TILE as u32);
        if !(min_x..=min_x + 1).contains(&tiles_x) || !(min_y..=min_y + 1).contains(&tiles_y) {
            return Err(OzfError::Corrupt(format!(
                "число тайлов {tiles_x}x{tiles_y} не сходится с размером {width}x{height}"
            )));
        }
        let mut pal_raw = [0u8; 1024];
        file.read_exact(&mut pal_raw)?;
        let palette = (0..256)
            .map(|i| [pal_raw[i * 4 + 2], pal_raw[i * 4 + 1], pal_raw[i * 4]]) // BGR0 -> RGB
            .collect();

        let count = (tiles_x * tiles_y + 1) as usize;
        let mut t = vec![0u8; count * 4];
        file.read_exact(&mut t)?;
        let tile_offsets: Vec<u32> = (0..count).map(|i| u32_at(&t, i * 4)).collect();
        if tile_offsets.windows(2).any(|w| w[0] > w[1]) || tile_offsets[count - 1] as u64 > size {
            return Err(OzfError::Corrupt("смещения тайлов не по возрастанию".into()));
        }
        Ok(Level { width, height, tiles_x, tiles_y, palette, tile_offsets })
    }

    /// Индексы уровней от крупного масштаба к мелкому (по ширине).
    pub fn levels_by_size(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.levels.len()).collect();
        v.sort_by_key(|&i| std::cmp::Reverse(self.levels[i].width));
        v
    }

    /// Тайл как 64×64 индексов палитры, строки сверху вниз.
    pub fn tile_indices(&mut self, level: usize, tx: u32, ty: u32) -> Result<Vec<u8>> {
        let lv = self.levels.get(level).ok_or(OzfError::NoLevel(level))?;
        if tx >= lv.tiles_x || ty >= lv.tiles_y {
            return Err(OzfError::NoTile(tx, ty));
        }
        let i = (ty * lv.tiles_x + tx) as usize;
        let (a, b) = (lv.tile_offsets[i] as u64, lv.tile_offsets[i + 1] as u64);
        let mut z = vec![0u8; (b - a) as usize];
        self.file.seek(SeekFrom::Start(a))?;
        self.file.read_exact(&mut z)?;
        let mut raw = Vec::with_capacity(TILE * TILE);
        ZlibDecoder::new(&z[..])
            .read_to_end(&mut raw)
            .map_err(|e| OzfError::Corrupt(format!("zlib: {e}")))?;
        if raw.len() != TILE * TILE {
            return Err(OzfError::Corrupt(format!("тайл {} байт вместо 4096", raw.len())));
        }
        // В файле строки идут снизу вверх.
        let mut out = vec![0u8; TILE * TILE];
        for r in 0..TILE {
            out[r * TILE..(r + 1) * TILE]
                .copy_from_slice(&raw[(TILE - 1 - r) * TILE..(TILE - r) * TILE]);
        }
        Ok(out)
    }

    /// Тайл в RGBA (64×64×4).
    pub fn tile_rgba(&mut self, level: usize, tx: u32, ty: u32) -> Result<Vec<u8>> {
        let idx = self.tile_indices(level, tx, ty)?;
        let pal = &self.levels[level].palette;
        let mut out = Vec::with_capacity(TILE * TILE * 4);
        for p in idx {
            let c = pal[p as usize];
            out.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        Ok(out)
    }

    /// Целый уровень в RGB (width*height*3). Для проверок и маленьких уровней.
    pub fn level_rgb(&mut self, level: usize) -> Result<(u32, u32, Vec<u8>)> {
        let (w, h, tx_n, ty_n) = {
            let l = self.levels.get(level).ok_or(OzfError::NoLevel(level))?;
            (l.width as usize, l.height as usize, l.tiles_x, l.tiles_y)
        };
        let mut img = vec![0u8; w * h * 3];
        for ty in 0..ty_n {
            for tx in 0..tx_n {
                let rgba = self.tile_rgba(level, tx, ty)?;
                for r in 0..TILE {
                    let y = ty as usize * TILE + r;
                    if y >= h { break; }
                    for c in 0..TILE {
                        let x = tx as usize * TILE + c;
                        if x >= w { break; }
                        let s = (r * TILE + c) * 4;
                        let d = (y * w + x) * 3;
                        img[d..d + 3].copy_from_slice(&rgba[s..s + 3]);
                    }
                }
            }
        }
        Ok((w as u32, h as u32, img))
    }
}
