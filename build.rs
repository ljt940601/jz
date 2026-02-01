use std::path::Path;
use std::fs::File;
use std::io::{BufWriter, Write};

fn main() {
    #[cfg(windows)]
    {
        let png_path = "icon.png";
        let ico_path = "icon.ico";

        // 将 PNG 转换为 ICO
        if Path::new(png_path).exists() {
            convert_png_to_ico(png_path, ico_path);
        }

        // 嵌入图标资源
        if Path::new(ico_path).exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon(ico_path);
            res.compile().unwrap();
        }
    }
}

#[cfg(windows)]
fn convert_png_to_ico(png_path: &str, ico_path: &str) {
    let img = image::open(png_path).expect("Failed to open PNG");

    // 生成多个尺寸
    let sizes = [256, 64, 48, 32, 16];
    let mut images: Vec<(u32, Vec<u8>)> = Vec::new();

    for size in sizes {
        let resized = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
        let rgba = resized.to_rgba8();
        images.push((size, rgba.into_raw()));
    }

    // 写入 ICO 文件
    let file = File::create(ico_path).expect("Failed to create ICO");
    let mut writer = BufWriter::new(file);

    // ICO 文件头
    writer.write_all(&[0, 0]).unwrap(); // 保留
    writer.write_all(&[1, 0]).unwrap(); // ICO 类型
    writer.write_all(&(images.len() as u16).to_le_bytes()).unwrap(); // 图像数量

    // 计算偏移量
    let header_size = 6 + images.len() * 16;
    let mut offset = header_size;
    let mut image_data: Vec<Vec<u8>> = Vec::new();

    for (size, pixels) in &images {
        let size = *size;

        // 创建 BMP 数据（不含文件头）
        let mut bmp_data = Vec::new();

        // BITMAPINFOHEADER (40 bytes)
        bmp_data.extend_from_slice(&40u32.to_le_bytes()); // 头大小
        bmp_data.extend_from_slice(&(size as i32).to_le_bytes()); // 宽
        bmp_data.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // 高（包含掩码）
        bmp_data.extend_from_slice(&1u16.to_le_bytes()); // 平面数
        bmp_data.extend_from_slice(&32u16.to_le_bytes()); // 位深度
        bmp_data.extend_from_slice(&0u32.to_le_bytes()); // 压缩
        bmp_data.extend_from_slice(&((size * size * 4) as u32).to_le_bytes()); // 图像大小
        bmp_data.extend_from_slice(&0u32.to_le_bytes()); // X 分辨率
        bmp_data.extend_from_slice(&0u32.to_le_bytes()); // Y 分辨率
        bmp_data.extend_from_slice(&0u32.to_le_bytes()); // 颜色数
        bmp_data.extend_from_slice(&0u32.to_le_bytes()); // 重要颜色数

        // 像素数据（从下到上，BGRA 格式）
        for y in (0..size).rev() {
            for x in 0..size {
                let idx = ((y * size + x) * 4) as usize;
                bmp_data.push(pixels[idx + 2]); // B
                bmp_data.push(pixels[idx + 1]); // G
                bmp_data.push(pixels[idx]);     // R
                bmp_data.push(pixels[idx + 3]); // A
            }
        }

        image_data.push(bmp_data);
    }

    // 写入图像目录
    for (i, (size, _)) in images.iter().enumerate() {
        let size = *size;
        let w = if size >= 256 { 0u8 } else { size as u8 };
        let h = w;
        writer.write_all(&[w, h]).unwrap(); // 宽高
        writer.write_all(&[0]).unwrap(); // 调色板
        writer.write_all(&[0]).unwrap(); // 保留
        writer.write_all(&1u16.to_le_bytes()).unwrap(); // 颜色平面
        writer.write_all(&32u16.to_le_bytes()).unwrap(); // 位深度
        writer.write_all(&(image_data[i].len() as u32).to_le_bytes()).unwrap(); // 数据大小
        writer.write_all(&(offset as u32).to_le_bytes()).unwrap(); // 偏移
        offset += image_data[i].len();
    }

    // 写入图像数据
    for data in &image_data {
        writer.write_all(data).unwrap();
    }
}
