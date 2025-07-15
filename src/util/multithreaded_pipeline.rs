use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle, yield_now},
    time::Duration,
};

/// The multithreaded pipeline takes a serial list of inputs, distributes
/// each input to a thread, and combines them back into the same order
/// of the inputs.
pub struct MultithreadPipeline<I: Sync + Send, O: Sync + Send, C> {
    next_input: Arc<Mutex<Option<(DataOrCommand<I>, usize)>>>,
    next_input_index: usize,
    // keeps track to ensure completion of work before terminating
    number_outputs_handled: usize,
    output_context: C,
    output_handler: Box<dyn FnMut(&mut C, O)>,
    output: Arc<Mutex<OutputBuffer<O>>>,
    threads: Vec<JoinHandle<()>>,
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
            next_input: Arc::new(Mutex::new(None)),
            next_input_index: 0,
            number_outputs_handled: 0,
            output: Arc::new(Mutex::new(OutputBuffer {
                offset: 0,
                buffer: VecDeque::new(),
            })),
            threads: Vec::new(),
            output_context,
            output_handler,
        }
    }

    /// Writes an input to the pipeline. Will wait until the next input is writeable.
    /// This method should only be called by one thread.
    pub fn write(&mut self, input: I) {
        self._write(DataOrCommand::Data(input));
    }

    fn _write(&mut self, input: DataOrCommand<I>) {
        // keep waiting if future tasks are being finished too fast, keep buffer size down
        // // todo make 8 not hard-coded
        // while self.output.lock().unwrap().buffer.len() > 8 {
        //     self.poll();
        //     yield_now();
        // }

        let index = self.next_input_index;
        self.next_input_index += 1;

        loop {
            let mut next_input = self.next_input.lock().unwrap();
            if next_input.is_none() {
                let _ = next_input.insert((input, index));
                break;
            }
            drop(next_input);
            yield_now();
        }
    }

    /// Polls the output buffer to check if there are any new outputs to handle.
    pub fn poll(&mut self) {
        while let Some(res) = self.read() {
            (self.output_handler)(&mut self.output_context, res);
        }
    }

    /// Keeps polling until the last output has been handled. Will busy-wait.
    pub fn finalize(mut self) -> C {
        let number_inputs = self.next_input_index;

        for _ in 0..self.threads.len() {
            self._write(DataOrCommand::Terminate);
        }

        while self.number_outputs_handled < number_inputs {
            self.poll();
            yield_now();
        }

        return self.output_context;
    }

    fn read(&mut self) -> Option<O> {
        let mut output = self.output.lock().unwrap();
        if output.buffer.is_empty() {
            return None;
        }
        let next_item = output.buffer.get(0)?;
        if next_item.is_none() {
            return None;
        }

        let next_item = output.buffer.pop_front()?;
        output.offset += 1;
        self.number_outputs_handled += 1;
        return next_item;
    }

    pub fn spawn_workers<Init: Send + Clone + 'static>(
        &mut self,
        num_workers: usize,
        init: Init,
        process_fn: impl Fn(&Init, I) -> O + Sync + Send + Copy + 'static,
    ) {
        for _ in 0..num_workers {
            let next_input_lock = Arc::clone(&self.next_input);
            let output_lock = Arc::clone(&self.output);
            let thread_init = init.clone();

            self.threads.push(thread::spawn(move || {
                loop {
                    let mut next_input_unlocked = next_input_lock.lock().unwrap();
                    let next_input = next_input_unlocked.take();
                    drop(next_input_unlocked);

                    if let Some((DataOrCommand::Data(input_data), input_index)) = next_input {
                        let output = process_fn(&thread_init, input_data);

                        let mut output_unlocked = output_lock.lock().unwrap();
                        let output_index = input_index - output_unlocked.offset;
                        while output_unlocked.buffer.len() <= output_index {
                            output_unlocked.buffer.push_back(None);
                        }
                        output_unlocked.buffer[output_index].replace(output);
                        {
                            let buf_len = output_unlocked.buffer.len();
                            if buf_len > 8 {
                                println!("Warn output buff length is larger: {}", buf_len);
                            }
                        }
                    } else if let Some((DataOrCommand::Terminate, _)) = next_input {
                        return;
                    } else {
                        yield_now();
                    }
                }
            }));
        }
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
            // println!("{} + 1", x);
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
