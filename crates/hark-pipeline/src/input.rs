//! Observe hotkey edges while transcription blocks on another worker.
//! Admission covers the entire hold and completion, including queued edges.
use crate::lifecycle::RunControl;
use hark_audio::ring::Consumer;
use hark_hotkey::PttEvent;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

pub(crate) struct Edge {
    pub event: PttEvent,
    pub at_abs: u64,
}

#[derive(Default)]
struct Admission {
    /// Some(false) consumes the rest of a hold rejected while busy, even if
    /// transcription finishes before that hold's release.
    held: Option<bool>,
}

impl Admission {
    fn observe(&mut self, event: PttEvent, at_abs: u64, run: &RunControl) -> Option<Edge> {
        if run.is_cancelled() {
            return None;
        }
        let accepted = match event {
            PttEvent::Down => {
                if self.held.is_some() {
                    return None;
                }
                let accepted = run.start_cycle();
                self.held = Some(accepted);
                accepted
            }
            PttEvent::Up | PttEvent::UpMissed => self.held.take() == Some(true),
            PttEvent::Intercepted(_) => true,
        };
        accepted.then_some(Edge { event, at_abs })
    }
}

pub(crate) fn route(
    input: Receiver<PttEvent>,
    clock: Consumer,
    run: Arc<RunControl>,
) -> std::io::Result<Receiver<Edge>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hark-dictation-input".into())
        .spawn(move || {
            let mut admission = Admission::default();
            for event in input {
                if run.is_cancelled() {
                    break;
                }
                if let Some(edge) = admission.observe(event, clock.total_written(), &run) {
                    if tx.send(edge).is_err() {
                        break;
                    }
                }
            }
        })?;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_edges_keep_observed_positions_when_consumed_later() {
        let run = RunControl::default();
        let mut admission = Admission::default();
        let down = admission.observe(PttEvent::Down, 10, &run).unwrap();
        let up = admission.observe(PttEvent::Up, 90, &run).unwrap();
        assert_eq!((down.at_abs, up.at_abs), (10, 90));
        assert!(
            !run.start_cycle(),
            "queued work is already busy before the worker reads Up"
        );
    }

    #[test]
    fn a_busy_hold_stays_rejected_when_the_worker_finishes_before_release() {
        let run = RunControl::default();
        let mut admission = Admission::default();
        assert!(admission.observe(PttEvent::Down, 0, &run).is_some());
        assert!(admission.observe(PttEvent::Up, 10, &run).is_some());
        assert!(admission.observe(PttEvent::Down, 20, &run).is_none());
        run.finish_cycle();
        assert!(admission.observe(PttEvent::Down, 25, &run).is_none());
        assert!(admission.observe(PttEvent::UpMissed, 30, &run).is_none());
        assert!(admission.observe(PttEvent::Down, 40, &run).is_some());
        assert_eq!(
            admission.observe(PttEvent::Up, 50, &run).unwrap().at_abs,
            50
        );
    }

    #[test]
    fn complete_busy_cycles_are_discarded_before_queueing() {
        let run = RunControl::default();
        assert!(run.start_cycle());
        let mut admission = Admission::default();
        for _ in 0..10 {
            assert!(admission.observe(PttEvent::Down, 10, &run).is_none());
            assert!(admission.observe(PttEvent::Up, 20, &run).is_none());
        }
        run.finish_cycle();
        assert!(admission.observe(PttEvent::Down, 30, &run).is_some());
    }

    #[test]
    fn threaded_observer_keeps_reading_while_completion_is_busy() {
        let (producer, consumer) = hark_audio::ring::ring(1_000);
        let run = Arc::new(RunControl::default());
        let (tx, input) = mpsc::channel();
        let output = route(input, consumer, run.clone()).unwrap();
        tx.send(PttEvent::Down).unwrap();
        assert_eq!(output.recv().unwrap().at_abs, 0);
        producer.push(&[0.2; 100]);
        tx.send(PttEvent::Up).unwrap();
        assert_eq!(output.recv().unwrap().at_abs, 100);

        tx.send(PttEvent::Down).unwrap();
        // Advisory events are an ordered barrier proving the busy Down
        // was consumed before completion, without timing assumptions.
        let warning = PttEvent::Intercepted(hark_hotkey::PttKeyCode::LCtrl);
        tx.send(warning).unwrap();
        assert!(matches!(
            output.recv().unwrap().event,
            PttEvent::Intercepted(_)
        ));
        run.finish_cycle();
        producer.push(&[0.2; 100]);
        tx.send(PttEvent::Up).unwrap();
        tx.send(PttEvent::Down).unwrap();
        let next = output.recv().unwrap();
        assert!(matches!(next.event, PttEvent::Down));
        assert_eq!(next.at_abs, 200);
        drop(tx);
        assert!(output.recv().is_err());
    }
}
