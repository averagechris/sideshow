use image::imageops::FilterType;
#[cfg(feature = "avif")]
use image::{ExtendedColorType, ImageEncoder};
use std::env;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

fn usage() -> ! {
    eprintln!(
        "usage: sideshow-avif-evaluation-encoder \
         <reference|webp|avif> <input> <output> <quality> <speed> <iterations> <max-dim>"
    );
    std::process::exit(2);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 7 {
        usage();
    }
    let format = &args[0];
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    let quality = args[3].parse::<u8>()?;
    let speed = args[4].parse::<u8>()?;
    let iterations = args[5].parse::<usize>()?;
    let max_dim = args[6].parse::<u32>()?;
    if iterations == 0 || max_dim == 0 {
        return Err("iterations and max-dim must be greater than zero".into());
    }

    let source = fs::read(input)?;
    let mut image = image::load_from_memory(&source)?;
    let longest = image.width().max(image.height());
    if longest > max_dim {
        let scale = max_dim as f64 / longest as f64;
        image = image.resize(
            ((image.width() as f64 * scale).round() as u32).max(1),
            ((image.height() as f64 * scale).round() as u32).max(1),
            FilterType::Lanczos3,
        );
    }

    if format == "reference" {
        image.save_with_format(output, image::ImageFormat::Png)?;
        println!(
            "{{\"format\":\"reference\",\"width\":{},\"height\":{}}}",
            image.width(),
            image.height()
        );
        return Ok(());
    }

    // Warm up once, then report the median of identical encode-only runs. Decode,
    // resize, and output I/O are deliberately outside the timed region for both codecs.
    let _ = encode(format, &image, quality, speed)?;
    let mut samples = Vec::with_capacity(iterations);
    let mut encoded = Vec::new();
    for _ in 0..iterations {
        let started = Instant::now();
        encoded = encode(format, &image, quality, speed)?;
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    fs::write(output, &encoded)?;
    let median = samples[samples.len() / 2];
    println!(
        "{{\"format\":\"{}\",\"quality\":{},\"speed\":{},\"threads\":1,\
         \"iterations\":{},\"width\":{},\"height\":{},\"encode_median_ms\":{:.3},\
         \"output_bytes\":{}}}",
        format,
        quality,
        speed,
        iterations,
        image.width(),
        image.height(),
        millis(median),
        encoded.len()
    );
    Ok(())
}

fn encode(
    format: &str,
    image: &image::DynamicImage,
    quality: u8,
    _speed: u8,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    match format {
        "webp" => Ok(webp::Encoder::from_image(image)
            .map_err(|error| format!("webp encode setup failed: {error}"))?
            .encode(f32::from(quality))
            .to_vec()),
        #[cfg(feature = "avif")]
        "avif" => {
            let rgba = image.to_rgba8();
            let mut output = Vec::new();
            image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut output, _speed, quality)
                .with_num_threads(Some(1))
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    ExtendedColorType::Rgba8,
                )?;
            Ok(output)
        }
        #[cfg(not(feature = "avif"))]
        "avif" => Err("AVIF support was not enabled at build time".into()),
        _ => Err(format!("unknown format: {format}").into()),
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}
