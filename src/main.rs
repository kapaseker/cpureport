use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use chrono::Local;
use clap::Parser;
use rust_xlsxwriter::{RowNum, Workbook};

/// Args
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// device id, if not set, just `adb -d`, if set, `adb -s [device]`
    #[arg(short, long)]
    device: Option<String>,

    /// app's package to test
    #[arg(short, long)]
    package: String,

    /// test duration (seconds)
    #[arg(short, long)]
    time: Option<u64>
}

// Function to get the current time as a formatted string
fn get_current_time() -> String {
    Local::now().format("%Y%m%d_%H%M%S").to_string()
}

// Function to run adb commands and capture the output
fn run_adb_command(command: &str) -> String {
    
    let mut cmd = if cfg!(target_os = "windows") { 
        let mut win_cmd = Command::new("cmd");
        win_cmd.arg("/C");
        win_cmd
    } else { 
        let mut sh_cmd = Command::new("sh");
        sh_cmd.arg("-c");
        sh_cmd
    };
    
    let output = cmd
        .arg(command)
        .output()
        .expect("Failed to execute adb command");
    String::from_utf8_lossy(&output.stdout).to_string()
}

// Function to collect CPU data
fn get_cpu_data(cpu_list: Arc<Mutex<Vec<f64>>>, device:&str, end_time: u64, pkg: &str) {
    while SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        < end_time
    {
        let top_result = run_adb_command(&format!("adb {} shell top -b -n 1 | grep {}", device, pkg));
        if let Some(cpu_line) = top_result.lines().next() {
            let cpu_value: f64 = cpu_line
                .split_whitespace()
                .nth(8)
                .unwrap_or("0")
                .replace("%", "")
                .parse()
                .unwrap_or(0.0);
            println!("CPU: {}", cpu_value);
            cpu_list.lock().unwrap().push(cpu_value);
        }
        thread::sleep(Duration::from_secs(1));
    }
    cpu_list.lock().unwrap().remove(0); // Remove the first anomalous value
}

// Function to collect memory data
fn get_mem_data(mem_list: Arc<Mutex<Vec<f64>>>, device:&str, end_time: u64, pkg: &str) {

    while SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        < end_time
    {
        let mem_result = run_adb_command(&format!("adb {} shell dumpsys meminfo {}", device, pkg));
        mem_result.lines().for_each(|line| {
            if line.contains("TOTAL PSS:") {
                // println!("{}", line);
                let pss_memory = line.split_whitespace().collect::<Vec<&str>>().get(2).unwrap_or(&"0").parse().unwrap_or(0.0);
                println!("Mem: {}", pss_memory);
                mem_list.lock().unwrap().push(pss_memory);
            }
        });
        thread::sleep(Duration::from_secs(3));
    }

    // 通常执行脚本第一个数据异常的高，移除第一个数据
    mem_list.lock().unwrap().remove(0);
}

// Main function
fn main() {

    let args = Args::parse();
    let pkg = args.package;
    let device = args.device.unwrap_or("".to_string());
    let duration = args.time.unwrap_or(60);

    println!("测试包名为: {}", pkg);

    let device_cmd = if device.is_empty() {
        println!("不指定设备");
        String::from("-d")
    } else {
        println!("指定设备为: {}", device);
        format!("-s {}", device)
    };

    let end_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + duration;

    println!("测试时长为: {}", duration);
    println!("结束时间为: {}", end_time);

    let f_path = ".";

    let cpu_list = Arc::new(Mutex::new(Vec::new()));
    let mem_list = Arc::new(Mutex::new(Vec::new()));

    // Spawn threads for CPU and memory data collection
    let cpu_thread = {
        let cpu_list = Arc::clone(&cpu_list);
        let pkg = pkg.clone();
        let device_cmd = device_cmd.clone();
        thread::spawn(move || get_cpu_data(cpu_list, &device_cmd, end_time, &pkg))
    };

    let mem_thread = {
        let mem_list = Arc::clone(&mem_list);
        let pkg = pkg.clone();
        let device_cmd = device_cmd.clone();
        thread::spawn(move || get_mem_data(mem_list, &device_cmd, end_time, &pkg))
    };

    // Wait for threads to finish
    cpu_thread.join().unwrap();
    mem_thread.join().unwrap();

    let current_time = get_current_time();

    println!("current time is: {}", current_time);

    // Save results to Excel files
    let cpu_file_path = format!("{}/cpu_data_{}.xlsx", f_path, current_time);
    let mem_file_path = format!("{}/mem_data_{}.xlsx", f_path, current_time);

    let cpu_data = cpu_list.lock().unwrap();
    let mem_data = mem_list.lock().unwrap();

    let cpu_sum = cpu_data.iter().sum::<f64>();

    let cpu_average: f64 = cpu_sum / cpu_data.len() as f64;
    let cpu_max = cpu_data.iter().max_by(|a, b| a.total_cmp(b)).unwrap_or(&0.0);

    let mem_sum = mem_data.iter().sum::<f64>();
    let mem_average: f64 = mem_sum / (mem_data.len() as f64 * 1024.0);
    let mem_max = mem_data.iter().max_by(|a, b| a.total_cmp(b)).unwrap_or(&0.0) / 1024.0;

    println!("cpu均值: {}", cpu_average);
    println!("cpu峰值: {}", cpu_max);
    println!("内存均值: {}", mem_average);
    println!("内存峰值: {}", mem_max);

    // Save CPU data
    {
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("Cpu Data").unwrap();
        cpu_data.iter().enumerate().for_each(|(idx, cpu)| {
            sheet.write(idx as RowNum, 1, cpu.to_string()).unwrap();
        });
        sheet.write_row(cpu_data.len() as RowNum, 0, ["Cpu Max", cpu_max.to_string().as_str()]).unwrap();
        sheet.write_row(cpu_data.len() as RowNum + 1, 0, ["Cpu Average", cpu_average.to_string().as_str()]).unwrap();

        workbook.save(&cpu_file_path).unwrap();
    }

    // Save Memory Data
    {
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("Memory Data").unwrap();
        mem_data.iter().enumerate().for_each(|(idx, memory)| {
            sheet.write(idx as RowNum, 1, memory.to_string()).unwrap();
        });

        sheet.write_row(mem_data.len() as RowNum, 0, ["Mem Max", mem_max.to_string().as_str()]).unwrap();
        sheet.write_row(mem_data.len() as RowNum + 1, 0, ["Mem Average", mem_average.to_string().as_str()]).unwrap();

        workbook.save(&mem_file_path).unwrap();
    }

    println!("Finished!");
}