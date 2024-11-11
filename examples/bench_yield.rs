#![feature(allocator_api)]

use std::{alloc::Global, hint::black_box, iter, time::Instant};

use futures_lite::future::yield_now;
use spin_on::spin_on;
use time::{ext::InstantExt, Duration};
use unico::asym::{sync, AsymWait};
use unico_context::{boost::Boost, global_resumer};
use unico_stack::global_stack_allocator;

global_resumer!(Boost);
global_stack_allocator!(Global);

#[inline(never)]
fn test(times: u32) -> Duration {
    let synced = spin_on(black_box(async {
        sync(|| {
            yield_now().wait();
            let start = Instant::now();
            for _ in 0..times {
                yield_now().wait();
            }
            Instant::now().signed_duration_since(start) / times
        })
        .await
    }));

    let direct = spin_on(black_box(async {
        yield_now().await;
        let start = Instant::now();
        for _ in 0..times {
            yield_now().await;
        }
        Instant::now().signed_duration_since(start) / times
    }));

    synced - direct
}

fn main() {
    const NUMS: &[u32] = &[
        1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144, 524288, 1048576,
    ];

    let sum = NUMS.iter().fold(Duration::ZERO, |acc, &num| {
        let repeat = 1048576 / num;

        let diff = iter::repeat_with(|| test(num))
            .take(repeat as usize)
            .sum::<Duration>()
            / repeat;

        println!("yield {} times: {}", num, diff);
        acc + diff
    });

    println!("avr: {}", sum / NUMS.len() as u32);
}
