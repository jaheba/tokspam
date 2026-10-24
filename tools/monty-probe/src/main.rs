//! Compile, and optionally execute, one source in a disposable process.
use std::{io::{self, Read}, time::Duration};

use monty::MontyRun;
use monty_types::{CompileOptions, PrintWriter, ResourceLimits, ResourceTracker, memory_limit_with_headroom};

#[global_allocator]
static ALLOC: monty_alloc::LimitedAllocator = monty_alloc::LimitedAllocator;

fn main() {
    let mut source = String::new();
    io::stdin().take(1024 * 1024 + 1).read_to_string(&mut source).unwrap();
    if source.len() > 1024 * 1024 {
        println!("source-limit");
        return;
    }

    let run = std::env::args().any(|arg| arg == "--run");
    let memory = 32 * 1024 * 1024;
    monty_alloc::set_hard_limit(memory_limit_with_headroom(Some(memory), false)).unwrap();

    match MontyRun::new(source, "<spam-fuzz>", vec![], CompileOptions::default()) {
        Ok(mut program) if run => {
            let limits = ResourceLimits::default()
                .max_memory(memory)
                .max_feed_duration(Duration::from_millis(500));
            match program.run(vec![], ResourceTracker::new(limits), PrintWriter::Disabled) {
                Ok(_) => println!("ok"),
                Err(error) => println!("run-error:{:?}", error.exc_type()),
            }
        }
        Ok(_) => println!("ok"),
        Err(error) => println!("compile-error:{:?}", error.exc_type()),
    }
}
