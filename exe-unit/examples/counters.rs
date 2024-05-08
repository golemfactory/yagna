use ya_counters::Counter;
use ya_counters::{CpuCounter, MemCounter};

fn main() {
    let mut v: Vec<Vec<u64>> = Vec::new();

    let mut cpu = CpuCounter::default();
    let mut mem = MemCounter::default();

    for i in 0..1000000 {
        v.push(vec![0, 1, 2, 3, 4, 5, 6, 7]);

        if i % 50000 == 0 {
            println!("CPU: {:?}", cpu.frame().unwrap());
            println!("MEM PEAK: {:?} B", mem.peak().unwrap());
        }
    }
}
