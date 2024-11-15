#![feature(allocator_api)]

use std::{alloc::Global, future::IntoFuture};

use criterion::{async_executor::AsyncExecutor, black_box, Criterion};
use spin_on::spin_on;
use tokio::runtime::Runtime;
use unico::{
    asym::{sync, AsymWait},
    context::{boost::Boost, global_resumer},
    stack::global_stack_allocator,
};

global_resumer!(Boost);
global_stack_allocator!(Global);

struct SpinOnExec;

impl AsyncExecutor for SpinOnExec {
    fn block_on<T>(&self, future: impl std::future::Future<Output = T>) -> T {
        spin_on(future)
    }
}

pub fn creation(c: &mut Criterion, rt: &Runtime) {
    let mut group = c.benchmark_group("creation");
    group
        .bench_function("raw", |b| {
            b.to_async(SpinOnExec).iter(|| black_box(async {}));
        })
        .bench_function("unico", |b| {
            b.to_async(SpinOnExec)
                .iter(|| black_box(sync(|| {}).into_future()))
        })
        .bench_function("raw+tokio", |b| {
            b.to_async(rt).iter(|| rt.spawn(black_box(async {})))
        })
        .bench_function("unico+tokio", |b| {
            b.to_async(rt)
                .iter(|| rt.spawn(black_box(sync(|| {}).into_future())))
        })
        .bench_function("thread", |b| {
            b.iter(|| black_box(std::thread::spawn(|| {})).join())
        });
    group.finish();
}

pub fn yielding(c: &mut Criterion, rt: &Runtime) {
    let mut group = c.benchmark_group("yielding");
    group
        .bench_function("raw", |b| {
            b.to_async(SpinOnExec).iter(futures_lite::future::yield_now);
        })
        .bench_function("unico", |b| {
            spin_on(
                sync(|| {
                    b.iter(|| futures_lite::future::yield_now().wait());
                })
                .into_future(),
            );
        })
        .bench_function("tokio", |b| {
            b.to_async(rt).iter(tokio::task::yield_now);
        })
        .bench_function("unico+tokio", |b| {
            rt.block_on(
                sync(|| {
                    b.iter(|| tokio::task::yield_now().wait());
                })
                .into_future(),
            );
        })
        .bench_function("thread", |b| {
            b.iter(std::thread::yield_now);
        });
    group.finish();
}

fn main() {
    let mut c = Criterion::default();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    // creation(&mut c, &rt);
    yielding(&mut c, &rt);
}
