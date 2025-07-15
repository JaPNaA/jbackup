use std::{
    collections::VecDeque,
    sync::mpsc,
    thread::{self, JoinHandle},
    usize,
};

/// The multithreaded pipeline takes a serial list of inputs, distributes
/// each input to a thread, and combines them back into the same order
/// of the inputs.
pub struct MultithreadPipeline<I: Sync + Send, O: Sync + Send, C> {
    next_input_index: usize,
    // keeps track to ensure completion of work before terminating
    number_outputs_read: usize,
    output_context: C,
    output_handler: Box<dyn FnMut(&mut C, O)>,
    output: OutputBuffer<O>,
    // Tuples: Output, input index, thread index
    output_channel: (
        mpsc::Sender<(O, usize, usize)>,
        mpsc::Receiver<(O, usize, usize)>,
    ),
    threads: Vec<ThreadState<I>>,
}

struct ThreadState<I: Sync + Send> {
    input_channel: mpsc::Sender<(DataOrCommand<I>, usize)>,
    is_working: bool,
    join_handle: JoinHandle<()>,
}

struct OutputBuffer<O> {
    offset: usize,
    /// Buffer with the 0th item being the next item to return in the pipeline.
    buffer: VecDeque<Option<O>>,
}

enum DataOrCommand<I> {
    Data(I),
    Terminate,
}

impl<I: Sync + Send + 'static, O: Sync + Send + 'static, C> MultithreadPipeline<I, O, C> {
    pub fn new(output_context: C, output_handler: Box<dyn FnMut(&mut C, O)>) -> Self {
        Self {
            next_input_index: 0,
            number_outputs_read: 0,
            output_channel: mpsc::channel(),
            output: OutputBuffer {
                offset: 0,
                buffer: VecDeque::new(),
            },
            threads: Vec::new(),
            output_context,
            output_handler,
        }
    }

    /// Writes an input to the pipeline. Will wait until the next input is writeable.
    /// This method should only be called by one thread.
    pub fn write(&mut self, input: I) {
        let index = self.next_input_index;
        self.next_input_index += 1;

        loop {
            for thread in &mut self.threads {
                if !thread.is_working {
                    thread.is_working = true;
                    thread
                        .input_channel
                        .send((DataOrCommand::Data(input), index))
                        .unwrap();
                    return;
                }
            }

            self.poll_blocking();
        }
    }

    /// Polls the output buffer to check if there are any new outputs to handle.
    pub fn poll(&mut self) {
        self.read_to_buffer();
        self.flush_buffer();
    }

    /// Does not return until a thread finishes. If finished block is not the
    /// next block (finished out-of-order), the block will be added to the buffer
    /// and no processing will happen.
    pub fn poll_blocking(&mut self) {
        self.read_to_buffer_blocking();
        self.flush_buffer();
    }

    /// Keeps polling until the last output has been handled. Will busy-wait.
    pub fn finalize(mut self) -> C {
        let number_inputs = self.next_input_index;

        for thread in &self.threads {
            thread
                .input_channel
                .send((DataOrCommand::Terminate, 0))
                .unwrap();
        }

        while self.number_outputs_read < number_inputs {
            self.poll_blocking();
        }

        for thread in self.threads {
            thread.join_handle.join().unwrap();
        }

        return self.output_context;
    }

    pub fn spawn_workers<Init: Send + Clone + 'static>(
        &mut self,
        num_workers: usize,
        init: Init,
        process_fn: impl Fn(&Init, I) -> O + Sync + Send + Copy + 'static,
    ) {
        for _ in 0..num_workers {
            let thread_init = init.clone();

            let (input_tx, input_rx) = mpsc::channel();
            let output_tx = self.output_channel.0.clone();
            let thread_index = self.threads.len();

            let join_handle = thread::spawn(move || {
                loop {
                    let next_input = input_rx.recv().unwrap();

                    match next_input {
                        (DataOrCommand::Data(input_data), input_index) => {
                            if let Err(err) = output_tx.send((
                                process_fn(&thread_init, input_data),
                                input_index,
                                thread_index,
                            )) {
                                panic!("{}", err);
                            }
                        }
                        (DataOrCommand::Terminate, _) => return,
                    }
                }
            });

            self.threads.push(ThreadState {
                join_handle,
                is_working: false,
                input_channel: input_tx,
            });
        }
    }

    fn read_to_buffer(&mut self) {
        loop {
            let output = self.output_channel.1.try_recv();
            if let Ok(output_tuple) = output {
                self.process_output_tuple(output_tuple);
            } else {
                break;
            }
        }
    }

    fn read_to_buffer_blocking(&mut self) {
        let output = self.output_channel.1.recv().unwrap();
        self.process_output_tuple(output);
    }

    fn flush_buffer(&mut self) {
        while let Some(res) = self.try_read_from_buffer() {
            (self.output_handler)(&mut self.output_context, res);
        }
    }

    fn try_read_from_buffer(&mut self) -> Option<O> {
        if self.output.buffer.is_empty() {
            return None;
        }
        let next_item = self.output.buffer.get(0)?;
        if next_item.is_none() {
            return None;
        }

        let next_item = self.output.buffer.pop_front()?;
        self.output.offset += 1;
        self.number_outputs_read += 1;
        return next_item;
    }

    fn process_output_tuple(
        &mut self,
        (output_data, input_index, thread_index): (O, usize, usize),
    ) {
        self.threads[thread_index].is_working = false;

        let output_index = input_index - self.output.offset;
        while self.output.buffer.len() <= output_index {
            self.output.buffer.push_back(None);
        }
        self.output.buffer[output_index].replace(output_data);
    }
}

pub fn main() -> Result<(), String> {
    let mut mtp = MultithreadPipeline::<u32, u32, Box<u32>>::new(
        Box::new(1),
        Box::new(move |expected_next, res| {
            if res != **expected_next {
                eprintln!("Error: Got {} when expecting {}", res, expected_next);
            }
            **expected_next += 1;
        }),
    );

    mtp.spawn_workers(
        8,
        || {},
        |_, x| {
            return x + 1;
        },
    );

    for i in 0..1000 {
        mtp.poll();
        mtp.write(i);
    }

    let final_output = mtp.finalize();
    println!("{}", final_output);

    Ok(())
}
