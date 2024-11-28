#![feature(allocator_api)]

use std::{alloc::Global, io::Read};

use futures_lite::{AsyncRead, AsyncReadExt};
use spin_on::spin_on;
use unico::asym::{AsymWait, sync};
use unico_context::{boost::Boost, global_resumer};
use unico_stack::global_stack_allocator;

global_resumer!(Boost);
global_stack_allocator!(Global);

struct Synced<R>(R);

impl<R: AsyncRead + Unpin + Send> Read for Synced<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf).wait()
    }
}

async fn read_synced(
    r: &mut (impl AsyncRead + Unpin + Send),
    buf: &mut [u8],
) -> std::io::Result<usize> {
    sync(|| Synced(r).read(buf)).await
}

async fn read_direct(
    r: &mut (impl AsyncRead + Unpin + Send),
    buf: &mut [u8],
) -> std::io::Result<usize> {
    r.read(buf).await
}

fn main() {
    spin_on(async move {
        let r: &[u8] = &[0x12; 6];
        let mut buf1 = [0u8; 6];
        let mut buf2 = [0u8; 6];

        let s1 = read_synced(&mut { r }, &mut buf1).await.unwrap();
        assert_eq!(s1, 6);
        let s2 = read_direct(&mut { r }, &mut buf2).await.unwrap();
        assert_eq!(s2, 6);
        assert_eq!(buf1, buf2);
    })
}
