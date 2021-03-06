use crate::rt::{thread, Execution, FnBox};
use generator::{self, Generator, Gn};
use scoped_tls::scoped_thread_local;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;

pub struct Scheduler {
    /// Threads
    threads: Vec<Thread>,

    next_thread: usize,

    queued_spawn: VecDeque<Box<dyn FnBox>>,
}

type Thread = Generator<'static, Option<Box<dyn FnBox>>, ()>;

scoped_thread_local! {
    static STATE: RefCell<State<'_>>
}

struct State<'a> {
    execution: &'a mut Execution,
    queued_spawn: &'a mut VecDeque<Box<dyn FnBox>>,
}

impl Scheduler {
    /// Create an execution
    pub fn new(capacity: usize) -> Scheduler {
        let threads = spawn_threads(capacity);

        Scheduler {
            threads,
            next_thread: 0,
            queued_spawn: VecDeque::new(),
        }
    }

    /// Access the execution
    pub fn with_execution<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Execution) -> R,
    {
        STATE.with(|state| f(&mut state.borrow_mut().execution))
    }

    /// Perform a context switch
    pub fn switch() {
        generator::yield_with(());
    }

    pub fn spawn(f: Box<dyn FnBox>) {
        STATE.with(|state| {
            state.borrow_mut().queued_spawn.push_back(f);
        });
    }

    pub fn run<F>(&mut self, execution: &mut Execution, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.next_thread = 1;
        self.threads[0].set_para(Some(Box::new(f)));
        self.threads[0].resume();

        loop {
            if !execution.threads.is_active() {
                return;
            }

            let active = execution.threads.active_id();

            self.tick(active, execution);

            while let Some(th) = self.queued_spawn.pop_front() {
                let thread_id = self.next_thread;
                self.next_thread += 1;

                self.threads[thread_id].set_para(Some(th));
                self.threads[thread_id].resume();
            }
        }
    }

    fn tick(&mut self, thread: thread::Id, execution: &mut Execution) {
        let state = RefCell::new(State {
            execution: execution,
            queued_spawn: &mut self.queued_spawn,
        });

        let threads = &mut self.threads;

        STATE.set(unsafe { transmute_lt(&state) }, || {
            threads[thread.as_usize()].resume();
        });
    }
}

impl fmt::Debug for Scheduler {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Schedule")
            .field("threads", &self.threads)
            .finish()
    }
}

fn spawn_threads(n: usize) -> Vec<Thread> {
    (0..n)
        .map(|_| {
            let mut g = Gn::new(move || {
                loop {
                    let f: Option<Box<dyn FnBox>> = generator::yield_(()).unwrap();
                    generator::yield_with(());
                    f.unwrap().call();
                }

                // done!();
            });
            g.resume();
            g
        })
        .collect()
}

unsafe fn transmute_lt<'a, 'b>(state: &'a RefCell<State<'b>>) -> &'a RefCell<State<'static>> {
    ::std::mem::transmute(state)
}
