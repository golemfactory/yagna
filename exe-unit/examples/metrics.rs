use ya_exe_unit::counters::{CpuMetric, MemMetric};
use ya_counters::counters::Metric;

fn main() {
    let mut v: Vec<Vec<u64>> = Vec::new();

    let mut cpu = CpuMetric::default();
    let mut mem = MemMetric::default();

    for i in 0..1000000 {
        v.push(vec![0, 1, 2, 3, 4, 5, 6, 7]);

        if i % 50000 == 0 {
            println!("CPU: {:?}", cpu.frame().unwrap());
            println!("MEM PEAK: {:?} B", mem.peak().unwrap());
        }
    }
}
