//! LBTerm: a task is named by where it was spawned, for the slow-frame log
//! (lbterm_extras::slow). promise's spawn functions carry `#[track_caller]` and hand the site to
//! the scheduler with each run, so a task's line is its caller's, through spawn_into_new_thread
//! too.

use promise::spawn::{self, Runnable, SpawnSite};
use std::sync::Mutex;

static SCHEDULED: Mutex<Vec<(Runnable, SpawnSite)>> = Mutex::new(Vec::new());

fn record(runnable: Runnable, site: SpawnSite) {
    SCHEDULED.lock().unwrap().push((runnable, site));
}

#[test]
fn tasks_are_named_by_where_they_were_spawned() {
    spawn::set_schedulers(
        Box::new(|runnable: Runnable, site: SpawnSite| record(runnable, site)),
        Box::new(|runnable: Runnable, site: SpawnSite| record(runnable, site)),
    );

    let here = line!();
    let main = spawn::spawn_into_main_thread(async {});
    let low = spawn::spawn_into_main_thread_with_low_priority(async {});
    let local = spawn::spawn(async {});
    let local_low = spawn::spawn_with_low_priority(async {});
    let thread = spawn::spawn_into_new_thread(|| Ok(()));

    let scheduled: Vec<(Runnable, SpawnSite)> = SCHEDULED.lock().unwrap().drain(..).collect();
    let lines: Vec<u32> = scheduled.iter().map(|(_, site)| site.line()).collect();
    assert_eq!(lines, (here + 1..=here + 5).collect::<Vec<_>>());
    for (_, site) in &scheduled {
        assert!(
            site.file()
                .replace('\\', "/")
                .ends_with("promise/tests/lbterm_spawn_site.rs"),
            "{}",
            site.file()
        );
    }

    // Run here, on the thread the local ones were spawned on
    for (runnable, _) in scheduled {
        runnable.run();
    }
    drop((main, low, local, local_low, thread));
}
