use std::fs::File;
use std::io::{Read, Write, BufReader, BufWriter};
use std::path::Path;
use std::time::Duration;
use clap::{Parser, Subcommand};
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use csv::ReaderBuilder;
use anyhow::{Result, Context, bail};

#[derive(Debug, Clone)]
struct ColumnStats {
    name: String,
    min_val: f64,
    max_val: f64,
    sum: f64,
    count: u64,
    time_min: String,
    time_max: String,
}

impl ColumnStats {
    fn new(name: String, first_val: f64, first_time: String) -> Self {
        Self {
            name,
            min_val: first_val,
            max_val: first_val,
            sum: first_val,
            count: 1,
            time_min: first_time.clone(),
            time_max: first_time,
        }
    }

    fn update(&mut self, val: f64, time_str: &str) {
        self.sum += val;
        self.count += 1;
        if val < self.min_val {
            self.min_val = val;
            self.time_min = time_str.to_string();
        } else if val > self.max_val {
            self.max_val = val;
            self.time_max = time_str.to_string();
        }
    }

    fn avg(&self) -> f64 {
        self.sum / self.count as f64
    }
}


fn time_to_seconds(t: &str) -> Option<u32> {
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() != 3 { return None; }
    let h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let s: u32 = parts[2].parse().ok()?;
    Some(h * 3600 + m * 60 + s)
}

fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() { return None; }
    let normalized = s.replace(',', ".");
    normalized.parse::<f64>().ok()
}

#[derive(Parser)]
#[clap(name = "telemetry_tool", about = "Архиватор и анализатор телеметрии")]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Compress {
        file_name: String,
        file_archived: String,
    },
    Decompress {
        file_archived: String,
        file_result: String,
    },
    Analyze {
        file_name: String,
        report: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compress { file_name, file_archived } => {
            compress(&file_name, &file_archived)?;
        }
        Commands::Decompress { file_archived, file_result } => {
            decompress(&file_archived, &file_result)?;
        }
        Commands::Analyze { file_name, report } => {
            analyze(&file_name, &report)?;
        }
    }
    Ok(())
}

fn compress(src: &str, dst: &str) -> Result<()> {
    let mut input = File::open(src)
        .with_context(|| format!("Не удалось открыть файл для чтения: {}", src))?;
    let output = File::create(dst)
        .with_context(|| format!("Не удалось создать архив: {}", dst))?;
    let mut encoder = GzEncoder::new(output, Compression::default());

    let mut buffer = [0u8; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 { break; }
        encoder.write_all(&buffer[..n])?;
    }
    encoder.finish()?;
    println!("Сжатие завершено: {} -> {}", src, dst);
    Ok(())
}

fn decompress(src: &str, dst: &str) -> Result<()> {
    let input = File::open(src)
        .with_context(|| format!("Не удалось открыть архив: {}", src))?;
    let decoder = GzDecoder::new(input);
    let mut reader = BufReader::new(decoder);
    let mut output = File::create(dst)
        .with_context(|| format!("Не удалось создать файл: {}", dst))?;
    let mut buffer = [0u8; 65536];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 { break; }
        output.write_all(&buffer[..n])?;
    }
    println!("Восстановление завершено: {} -> {}", src, dst);
    Ok(())
}

fn analyze(csv_path: &str, report_path: &str) -> Result<()> {
    let file = File::open(csv_path)
        .with_context(|| format!("Не удалось открыть файл: {}", csv_path))?;
    let mut rdr = ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(true)
        .from_reader(file);

    let headers = rdr.headers()?.clone();
    let mut stats_vec: Vec<Option<ColumnStats>> = vec![None; headers.len()];
    
    for result in rdr.records() {
        let record = result?;
        let time_str = record.get(0).unwrap_or("").trim();
        if time_str.is_empty() {
            continue; 
        }

        for (idx, field) in record.iter().enumerate() {
            if idx == 0 { continue; } 
            if let Some(val) = parse_number(field) {
                let col_name = headers.get(idx).unwrap_or("").to_string();
                if let Some(stats) = &mut stats_vec[idx] {
                    stats.update(val, time_str);
                } else {
                    stats_vec[idx] = Some(ColumnStats::new(col_name, val, time_str.to_string()));
                }
            }
        }
    }

    let mut report = String::new();
    report.push_str("=== Отчёт по телеметрии ===\n");
    report.push_str(&format!("Файл: {}\n", csv_path));
    report.push_str("\n");
    for maybe_stats in stats_vec.iter().flatten() {
        report.push_str(&format!("Колонка: {}\n", maybe_stats.name));
        report.push_str(&format!("  Минимальное значение: {:.3}\n", maybe_stats.min_val));
        report.push_str(&format!("  Время минимума: {}\n", maybe_stats.time_min));
        report.push_str(&format!("  Максимальное значение: {:.3}\n", maybe_stats.max_val));
        report.push_str(&format!("  Время максимума: {}\n", maybe_stats.time_max));
        report.push_str(&format!("  Среднее значение: {:.3}\n", maybe_stats.avg()));
        report.push_str("\n");
    }
    if stats_vec.iter().all(|x| x.is_none()) {
        report.push_str("Не найдено числовых колонок для анализа.\n");
    }

    std::fs::write(report_path, report)
        .with_context(|| format!("Не удалось записать отчёт в файл: {}", report_path))?;
    println!("Анализ завершён. Результат сохранён в {}", report_path);
    Ok(())
}
