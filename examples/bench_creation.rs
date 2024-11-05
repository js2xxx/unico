#![feature(allocator_api)]

use std::{alloc::Global, hint::black_box, iter, time::Instant};

use spin_on::spin_on;
use time::{ext::InstantExt, Duration};
use unico::asym::sync;
use unico_context::{boost::Boost, global_resumer};
use unico_stack::global_stack_allocator;

global_resumer!(Boost);
global_stack_allocator!(Global);

#[inline(never)]
fn test(times: u32) -> Duration {
    let start = Instant::now();
    for _ in 0..times {
        spin_on(black_box(async {
            sync(|| {}).await;
        }));
    }
    let synced = Instant::now().signed_duration_since(start) / times;

    let start = Instant::now();
    for _ in 0..times {
        spin_on(black_box(async {}));
    }
    let direct = Instant::now().signed_duration_since(start) / times;

    synced - direct
}

fn main() {
    const NUMS: &[u32] = &[1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048];

    let sum = NUMS.iter().fold(Duration::ZERO, |acc, &num| {
        let repeat = 1048576 / num;

        let diff = iter::repeat_with(|| test(num))
            .take(repeat as usize)
            .sum::<Duration>()
            / repeat;

        println!("repeat {} times: {}", num, diff);
        acc + diff
    });

    println!("avr: {}", sum / NUMS.len() as u32);
}
