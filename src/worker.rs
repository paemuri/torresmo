use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::rpc::Rpc;
use crate::transmission::{Snapshot, TorrentDetails};

pub type Job = Box<dyn Send + FnOnce(&mut Rpc) -> Reply>;

pub enum Reply {
  Snapshot(Result<Snapshot, String>),
  // Boxed, as the details are much bigger than the other replies.
  Details(String, Result<Option<Box<TorrentDetails>>, String>),
  Done(Result<(), String>),
}

// Runs every RPC call on its own thread, one job at a time and in order, so a
// slow daemon never blocks the UI.
pub fn spawn(mut rpc: Rpc) -> (Sender<Job>, Receiver<Reply>) {
  let (job_sender, job_receiver) = mpsc::channel::<Job>();
  let (reply_sender, reply_receiver) = mpsc::channel();
  thread::spawn(move || {
    for job in job_receiver {
      if reply_sender.send(job(&mut rpc)).is_err() {
        break;
      }
    }
  });
  (job_sender, reply_receiver)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn jobs_run_in_order() {
    let (jobs, replies) = spawn(Rpc::new().unwrap());
    jobs.send(Box::new(|_| Reply::Done(Ok(())))).unwrap();
    jobs
      .send(Box::new(|_| Reply::Done(Err("second".to_owned()))))
      .unwrap();
    assert!(matches!(replies.recv().unwrap(), Reply::Done(Ok(()))));
    assert!(
      matches!(replies.recv().unwrap(), Reply::Done(Err(e)) if e == "second")
    );
  }
}
