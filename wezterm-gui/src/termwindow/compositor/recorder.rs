//! LBTerm: a stand-in for DirectComposition that records what the compositor gives it, so a test
//! can check the visual tree compositor.rs builds and what it animates with no window and no DWM
//! (lbterm/COMPOSITOR.md, "Checking it without a window", which has the design this follows and
//! the reader API a test uses).
//!
//! Each stand-in is a COM object made with the windows crate's `#[implement]`, so compositor.rs
//! calls it through the same interfaces, vtables and `cast()`s as it calls DirectComposition's own.
//! Every call that matters is written to one log, in order; a method compositor.rs doesn't call
//! fails with `E_NOTIMPL`, so the compositor's `?` turns it into the test's failure rather than
//! letting it pass unseen. Nothing here panics inside a COM method: a panic can't unwind out of
//! an `extern "system"` function, so it would abort the whole test binary.

use lbterm_extras::state::{millis, Played};
use lbterm_motion::{Cubic, Segments};
use std::collections::{BTreeMap, BTreeSet, HashMap};
// wezterm-gui is on the 2018 edition, whose prelude hasn't TryFrom
use std::convert::TryFrom;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use windows58::core::{implement, IUnknown, Interface, Result};
use windows58::Foundation::Numerics::Matrix3x2;
use windows58::Win32::Foundation::{BOOL, E_INVALIDARG, E_NOTIMPL, HANDLE, HWND};
use windows58::Win32::Graphics::Direct2D::Common::{
    D2D1_COLORMATRIX_ALPHA_MODE, D2D1_COLOR_F, D2D_MATRIX_4X4_F, D2D_MATRIX_5X4_F, D2D_RECT_F,
};
use windows58::Win32::Graphics::DirectComposition::*;
use windows58::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE, DXGI_FORMAT};

/// An object's number, given in the order they're made or first seen, from 1
pub type Id = u32;

/// What a call was given: one of the recorder's stand-ins, or something from outside it (a
/// swapchain wgpu made, which DirectComposition only shows), numbered the first time it's seen
/// and kept alive while the recorder is, as a visual keeps its content, so its number isn't
/// another's later
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Object {
    Ours(Id),
    Foreign(Id),
}

/// A place in the log: `Recorder::since` and `animated` read what came after it
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark(usize);

/// One piece of an animation, as `IDCompositionAnimation` is given it
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// `AddCubic(begin, c0, c1, c2, c3)`: lbterm_motion's own cubic, which DWM plays as
    /// `c0 + c1·τ + c2·τ² + c3·τ³` with τ the seconds since `begin`
    Cubic(Cubic),
    /// `AddRepeat(begin, duration)`: from `begin` on, the `duration` seconds before it again and
    /// again, until the next segment or the end
    Repeat { begin: f64, duration: f64 },
}

impl Segment {
    pub fn begin(&self) -> f64 {
        match self {
            Segment::Cubic(cubic) => cubic.begin,
            Segment::Repeat { begin, .. } => *begin,
        }
    }
}

/// An animation as it was when it was handed to a property: DirectComposition copies it at the
/// `Set` call, so what's added to the animation after doesn't change what the property plays
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    /// The animation object it was copied from
    pub animation: Id,
    /// In the order they were added, their beginnings never going back
    pub segments: Vec<Segment>,
    /// `End(at, value)`: `value` from `at` seconds on
    pub end: Option<(f64, f32)>,
}

impl Curve {
    /// Its value `t` seconds after the commit that starts it, as DWM plays it: the cubic `t` is
    /// in, a repeat's lap again, and the end's value from the end on. Before its first segment,
    /// where that starts
    pub fn value(&self, t: f64) -> f64 {
        match self.end {
            Some((at, value)) if t >= at => value as f64,
            _ => self.value_in(t, self.segments.len()),
        }
    }

    /// Its value at `t` from its first `upto` segments alone
    fn value_in(&self, t: f64, upto: usize) -> f64 {
        let segments = &self.segments[..upto];
        match segments.iter().rposition(|s| s.begin() <= t) {
            Some(i) => match segments[i] {
                Segment::Cubic(cubic) => cubic.value(t),
                // A repeat's time maps back into the `duration` before it, which only the
                // segments before it can hold, so this ends
                Segment::Repeat { begin, duration } => {
                    self.value_in(begin - duration + (t - begin).rem_euclid(duration), i)
                }
            },
            None => match segments.first() {
                Some(Segment::Cubic(cubic)) => cubic.coefficients[0] as f64,
                _ => self.end.map_or(0.0, |(_, value)| value as f64),
            },
        }
    }

    /// When it ends, in seconds from its start: `None` for one that plays for ever
    pub fn ends(&self) -> Option<f64> {
        self.end.map(|(at, _)| at)
    }

    /// How long a lap of its first repeat is: `Some` for a loop, which plays for ever
    pub fn lap(&self) -> Option<f64> {
        self.segments.iter().find_map(|s| match s {
            Segment::Repeat { duration, .. } => Some(*duration),
            Segment::Cubic(_) => None,
        })
    }

    /// Its cubics as lbterm_motion's `Segments`, what `drive` and `play` hand DWM, up to its end,
    /// or for a loop up to its first repeat with the value there: `None` for one that neither
    /// ends nor repeats
    pub fn segments(&self) -> Option<Segments> {
        let repeat = self
            .segments
            .iter()
            .position(|s| matches!(s, Segment::Repeat { .. }));
        let upto = repeat.unwrap_or(self.segments.len());
        let cubics = self.segments[..upto]
            .iter()
            .filter_map(|s| match s {
                Segment::Cubic(cubic) => Some(*cubic),
                Segment::Repeat { .. } => None,
            })
            .collect();
        let (end, end_value) = match repeat {
            Some(k) => {
                let at = self.segments[k].begin();
                (at, self.value_in(at, k) as f32)
            }
            None => self.end?,
        };
        Some(Segments {
            cubics,
            end,
            end_value,
        })
    }

    /// What it plays as numbers, worked out as the state file's are (`Segments::span`,
    /// lbterm/STATE.md's `played`) from what DWM was given: `ms` is how long it plays to its end
    /// or first repeat, the delay `drive` holds it for included, where the state file's starts
    /// after the delay
    pub fn played(&self) -> Option<Played> {
        let segments = self.segments()?;
        let span = segments.span();
        Some(Played {
            from: span.from,
            to: span.to,
            low: span.low,
            high: span.high,
            ms: millis(Duration::try_from_secs_f64(segments.end).unwrap_or_default()),
        })
    }
}

/// What a property holds: a value, or the copy of the animation it plays
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Value(f32),
    Animation(Curve),
}

impl Prop {
    pub fn value(&self) -> Option<f32> {
        match self {
            Prop::Value(value) => Some(*value),
            Prop::Animation(_) => None,
        }
    }

    pub fn curve(&self) -> Option<&Curve> {
        match self {
            Prop::Value(_) => None,
            Prop::Animation(curve) => Some(curve),
        }
    }

    /// What it shows `t` seconds after the commit that set it (`Tree::started`)
    pub fn at(&self, t: f64) -> f64 {
        match self {
            Prop::Value(value) => *value as f64,
            Prop::Animation(curve) => curve.value(t),
        }
    }
}

/// An animation handed to a property: `id`'s `property` plays `curve`
#[derive(Clone, Debug, PartialEq)]
pub struct Handed {
    pub id: Id,
    pub property: &'static str,
    pub curve: Curve,
}

/// One call, as the compositor made it. Properties are named as the method is without its
/// `Set` and the value overload's `2` ("OffsetX", "Opacity", "TopLeftRadiusX", "Angle"), a
/// colour matrix's elements by [`matrix`]
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// A stand-in made: `kind` is "device", "target", "visual", "animation", "clip",
    /// "colour matrix", "hue rotation", "rotate" or "scale"
    Made { id: Id, kind: &'static str },
    /// A property given a value
    Set {
        id: Id,
        property: &'static str,
        value: f32,
    },
    /// A property given an animation: the copy it plays
    Animated(Handed),
    /// `AddVisual(child, above, reference)`
    Add {
        parent: Id,
        child: Object,
        above: bool,
        reference: Option<Object>,
    },
    /// `RemoveVisual(child)`
    Remove { parent: Id, child: Object },
    /// An object linked to another or let go: a visual's "Content", "Clip", "Effect" or
    /// "Transform", a target's "Root"
    Linked {
        id: Id,
        property: &'static str,
        object: Option<Object>,
    },
    /// A segment added to an animation: `late` when it had been handed to a property already,
    /// which then doesn't play it
    Segment {
        id: Id,
        segment: Segment,
        late: bool,
    },
    /// `End(at, value)` on an animation, `late` as for a segment
    End {
        id: Id,
        at: f64,
        value: f32,
        late: bool,
    },
    /// `IDCompositionDevice::Commit`: what came before it shows, and every animation handed to
    /// a property since the last starts playing `at` it, as DWM starts them (`Tree::started`)
    Commit { at: Instant },
    /// A method the stand-in doesn't record, which failed with `E_NOTIMPL`
    Unrecorded { id: Id, method: &'static str },
}

impl Call {
    /// Whether it changes what DirectComposition shows once it's committed
    fn changes(&self) -> bool {
        matches!(
            self,
            Call::Set { .. }
                | Call::Animated(_)
                | Call::Add { .. }
                | Call::Remove { .. }
                | Call::Linked { .. }
        )
    }
}

/// The names of a colour matrix's elements, `matrix(row, column)`: DirectComposition's 5×4
/// matrix, rows 0 to 3 what each input channel (red, green, blue, alpha) gives each output
/// channel (the columns, in the same order), row 4 what's added to each
const MATRIX: [[&str; 4]; 5] = [
    ["Matrix[0][0]", "Matrix[0][1]", "Matrix[0][2]", "Matrix[0][3]"],
    ["Matrix[1][0]", "Matrix[1][1]", "Matrix[1][2]", "Matrix[1][3]"],
    ["Matrix[2][0]", "Matrix[2][1]", "Matrix[2][2]", "Matrix[2][3]"],
    ["Matrix[3][0]", "Matrix[3][1]", "Matrix[3][2]", "Matrix[3][3]"],
    ["Matrix[4][0]", "Matrix[4][1]", "Matrix[4][2]", "Matrix[4][3]"],
];

/// A colour matrix element's property name: `SetMatrixElement(row, column, ..)`'s
pub fn matrix(row: usize, column: usize) -> &'static str {
    MATRIX[row][column]
}

fn element(row: i32, column: i32) -> Result<&'static str> {
    let (Ok(row), Ok(column)) = (usize::try_from(row), usize::try_from(column)) else {
        return Err(invalid());
    };
    MATRIX
        .get(row)
        .and_then(|r| r.get(column))
        .copied()
        .ok_or_else(invalid)
}

fn invalid() -> windows58::core::Error {
    E_INVALIDARG.into()
}

/// What DirectComposition shows, or will once committed: every object's properties, what each
/// links, and the visual tree under each target's root
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    kinds: BTreeMap<Id, &'static str>,
    props: BTreeMap<(Id, &'static str), Prop>,
    links: BTreeMap<(Id, &'static str), Object>,
    /// Each visual's children, bottom to top
    children: BTreeMap<Id, Vec<Object>>,
    parent: BTreeMap<Object, Id>,
    /// Each animated property's commit, the moment DWM starts playing its animation: set at the
    /// Commit after it was handed, kept until the property is given something else
    started: BTreeMap<(Id, &'static str), Instant>,
}

impl Tree {
    /// Each target with its root
    pub fn roots(&self) -> Vec<(Id, Object)> {
        self.links
            .iter()
            .filter(|((_, property), _)| *property == "Root")
            .map(|((target, _), root)| (*target, *root))
            .collect()
    }

    /// The first target's root: the compositor has one target
    pub fn root(&self) -> Option<Object> {
        self.roots().first().map(|(_, root)| *root)
    }

    /// A visual's children, bottom to top
    pub fn children(&self, of: Object) -> Vec<Object> {
        match of {
            Object::Ours(id) => self.children.get(&id).cloned().unwrap_or_default(),
            Object::Foreign(_) => vec![],
        }
    }

    pub fn parent(&self, of: Object) -> Option<Object> {
        self.parent.get(&of).map(|id| Object::Ours(*id))
    }

    /// "visual", "clip" and so on (`Call::Made`), "foreign" for what isn't the recorder's
    pub fn kind(&self, of: Object) -> &'static str {
        match of {
            Object::Ours(id) => self.kinds.get(&id).copied().unwrap_or("unknown"),
            Object::Foreign(_) => "foreign",
        }
    }

    pub fn prop(&self, of: Object, property: &'static str) -> Option<&Prop> {
        match of {
            Object::Ours(id) => self.props.get(&(id, property)),
            Object::Foreign(_) => None,
        }
    }

    /// When an animated property's animation started playing: the moment of the Commit after
    /// it was handed over, which DWM starts it at, whenever the compositor worked it out
    /// (`None` for a value, or an animation not committed yet)
    pub fn started(&self, of: Object, property: &'static str) -> Option<Instant> {
        match of {
            Object::Ours(id) => self.started.get(&(id, property)).copied(),
            Object::Foreign(_) => None,
        }
    }

    /// What a property shows at `t` as DWM plays it: its value, or its animation from the
    /// commit that started it (`None` when it has neither, or its animation isn't committed)
    pub fn shows(&self, of: Object, property: &'static str, t: Instant) -> Option<f64> {
        match self.prop(of, property)? {
            Prop::Value(value) => Some(*value as f64),
            Prop::Animation(curve) => {
                let started = self.started(of, property)?;
                Some(curve.value(t.saturating_duration_since(started).as_secs_f64()))
            }
        }
    }

    /// A property's value, when it has one rather than an animation
    pub fn value(&self, of: Object, property: &'static str) -> Option<f32> {
        self.prop(of, property).and_then(Prop::value)
    }

    /// The animation a property plays, when it has one
    pub fn curve(&self, of: Object, property: &'static str) -> Option<&Curve> {
        self.prop(of, property).and_then(Prop::curve)
    }

    /// What an object links as `property` ("Content", "Clip", "Effect", "Transform", "Root")
    pub fn link(&self, of: Object, property: &'static str) -> Option<Object> {
        match of {
            Object::Ours(id) => self.links.get(&(id, property)).copied(),
            Object::Foreign(_) => None,
        }
    }

    /// Every property an object has, by name
    pub fn props(&self, of: Object) -> Vec<(&'static str, &Prop)> {
        match of {
            Object::Ours(id) => self
                .props
                .range((id, "")..(id.saturating_add(1), ""))
                .map(|((_, property), prop)| (*property, prop))
                .collect(),
            Object::Foreign(_) => vec![],
        }
    }

    /// Whether a visual is in a target's tree, and so on the screen
    pub fn shown(&self, of: Object) -> bool {
        let roots: Vec<Object> = self.roots().into_iter().map(|(_, root)| root).collect();
        self.lineage(of).iter().any(|o| roots.contains(o))
    }

    /// Every visual in the targets' trees, each after its parent and its children bottom to
    /// top, with how deep it is (a root 0)
    pub fn walk(&self) -> Vec<(usize, Object)> {
        let mut walked = vec![];
        for (_, root) in self.roots() {
            self.walk_from(root, 0, &mut walked);
        }
        walked
    }

    fn walk_from(&self, of: Object, depth: usize, walked: &mut Vec<(usize, Object)>) {
        walked.push((depth, of));
        for child in self.children(of) {
            self.walk_from(child, depth + 1, walked);
        }
    }

    /// `of`, its parent, and so up to the top: AddVisual keeps the tree free of cycles
    fn lineage(&self, of: Object) -> Vec<Object> {
        let mut lineage = vec![of];
        while let Some(parent) = self.parent(*lineage.last().unwrap()) {
            lineage.push(parent);
        }
        lineage
    }

    fn is_root(&self, of: Object) -> bool {
        self.roots().iter().any(|(_, root)| *root == of)
    }

    fn name(&self, of: Object) -> String {
        match of {
            Object::Ours(id) => format!("{} {id}", self.kind(of)),
            Object::Foreign(id) => format!("foreign {id}"),
        }
    }

    /// An object and its properties on a line: "visual 4 (OffsetX 3, Opacity 0 to 1 in 120 ms)"
    fn line(&self, of: Object) -> String {
        let props: Vec<String> = self
            .props(of)
            .into_iter()
            .map(|(property, prop)| format!("{property} {prop}"))
            .collect();
        if props.is_empty() {
            self.name(of)
        } else {
            format!("{} ({})", self.name(of), props.join(", "))
        }
    }

    fn write(&self, f: &mut fmt::Formatter, of: Object, depth: usize) -> fmt::Result {
        writeln!(f, "{:2$}{}", "", self.line(of), depth * 2)?;
        if let Object::Ours(id) = of {
            for ((_, property), linked) in self.links.range((id, "")..(id.saturating_add(1), "")) {
                writeln!(f, "{:3$}{}: {}", "", property, self.line(*linked), depth * 2 + 2)?;
            }
        }
        for child in self.children(of) {
            self.write(f, child, depth + 1)?;
        }
        Ok(())
    }
}

/// The tree as text, for a test's failure: each target, then each visual under it indented by
/// its depth, its children bottom to top, each line with its properties and under it what it
/// links with theirs
impl fmt::Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (target, root) in self.roots() {
            writeln!(f, "target {target}")?;
            self.write(f, root, 1)?;
        }
        Ok(())
    }
}

impl fmt::Display for Prop {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Prop::Value(value) => write!(f, "{}", num(*value as f64)),
            Prop::Animation(curve) => write!(f, "{curve}"),
        }
    }
}

/// "0 to 1 in 120 ms", with "(-0.02 to 1.04)" after for one that goes past its ends, "every"
/// for a loop's lap
impl fmt::Display for Curve {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Some(played) = self.played() else {
            return write!(f, "animation {} with no end", self.animation);
        };
        let how = if self.lap().is_some() { "every" } else { "in" };
        write!(f, "{} to {} {how} {} ms", num(played.from), num(played.to), played.ms)?;
        if played.low < played.from.min(played.to) || played.high > played.from.max(played.to) {
            write!(f, " ({} to {})", num(played.low), num(played.high))?;
        }
        Ok(())
    }
}

/// A number to three places with no zeros after
fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// An animation as it's built, before and after it's handed over
#[derive(Default)]
struct Building {
    segments: Vec<Segment>,
    end: Option<(f64, f32)>,
    handed: bool,
}

#[derive(Default)]
struct Log {
    calls: Vec<Call>,
    /// Each object's IUnknown, its COM identity, to what it is: a stand-in takes itself out as
    /// it goes, so its address made again by something else isn't taken for it
    known: HashMap<usize, Object>,
    /// The foreign objects seen, held as a visual holds its content
    held: BTreeMap<Id, IUnknown>,
    animations: HashMap<Id, Building>,
    /// The properties handed an animation since the last Commit, which starts them
    fresh: BTreeSet<(Id, &'static str)>,
    /// As the calls have left it, what isn't committed yet included
    live: Tree,
    /// As it stood at the last Commit
    committed: Tree,
    /// Where the calls since the last Commit start
    committed_at: usize,
    next: Id,
}

/// The stand-in device's log, shared by every stand-in it made
#[derive(Clone, Default)]
pub struct Recorder(Arc<Mutex<Log>>);

impl Recorder {
    /// A stand-in device, for `Compositor::with_device`: an `IDCompositionDesktopDevice` that
    /// casts to `IDCompositionDevice3` as DirectComposition's does
    pub fn device(&self) -> IDCompositionDesktopDevice {
        self.make("device", |me| Device { me }.into())
    }

    /// What DirectComposition shows: everything as it stood at the last Commit
    pub fn tree(&self) -> Tree {
        self.log().committed.clone()
    }

    /// Everything as the calls have left it, what isn't committed yet included
    pub fn live(&self) -> Tree {
        self.log().live.clone()
    }

    /// The calls since the last Commit that change what's shown: none once a frame is done, or
    /// what it changed shows late, at whatever commits next
    pub fn pending(&self) -> Vec<Call> {
        let log = self.log();
        log.calls[log.committed_at..]
            .iter()
            .filter(|c| c.changes())
            .cloned()
            .collect()
    }

    /// Where the log is now
    pub fn mark(&self) -> Mark {
        Mark(self.log().calls.len())
    }

    /// Every call since `mark`, in order
    pub fn since(&self, mark: Mark) -> Vec<Call> {
        self.log().calls.get(mark.0..).unwrap_or(&[]).to_vec()
    }

    /// Every call so far, in order
    pub fn calls(&self) -> Vec<Call> {
        self.log().calls.clone()
    }

    /// Each animation handed to a property since `mark`, in order
    pub fn animated(&self, mark: Mark) -> Vec<Handed> {
        self.since(mark)
            .into_iter()
            .filter_map(|c| match c {
                Call::Animated(handed) => Some(handed),
                _ => None,
            })
            .collect()
    }

    /// What the recorder knows `object` as, one of the compositor's own visuals say (`None` for
    /// what it hasn't been given)
    pub fn object(&self, object: &impl Interface) -> Option<Object> {
        let key = identity(object);
        self.log().known.get(&key).copied()
    }

    /// A foreign object the recorder holds, to cast to what it is (a swapchain)
    pub fn foreign(&self, object: Object) -> Option<IUnknown> {
        match object {
            Object::Foreign(id) => self.log().held.get(&id).cloned(),
            Object::Ours(_) => None,
        }
    }

    /// The log, whatever a test that failed holding it left it as
    fn log(&self) -> MutexGuard<'_, Log> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn make<I: Interface>(&self, kind: &'static str, make: impl FnOnce(Me) -> I) -> I {
        let id = {
            let mut log = self.log();
            log.next += 1;
            let id = log.next;
            log.calls.push(Call::Made { id, kind });
            log.live.kinds.insert(id, kind);
            if kind == "animation" {
                log.animations.insert(id, Building::default());
            }
            id
        };
        let made = make(Me {
            id,
            log: self.clone(),
        });
        let key = identity(&made);
        self.log().known.insert(key, Object::Ours(id));
        made
    }

    fn forget(&self, id: Id) {
        self.log().known.retain(|_, what| *what != Object::Ours(id));
    }

    /// One of the recorder's own of one of `kinds`; anything else fails with `E_INVALIDARG`, as
    /// DirectComposition fails an object another device made
    fn ours(&self, object: &impl Interface, kinds: &[&str]) -> Result<Id> {
        let key = identity(object);
        let log = self.log();
        match log.known.get(&key) {
            Some(Object::Ours(id)) if log.live.kinds.get(id).is_some_and(|k| kinds.contains(k)) => {
                Ok(*id)
            }
            _ => Err(invalid()),
        }
    }

    /// Something from outside the recorder, numbered and held the first time it's seen; one of
    /// the recorder's own fails with `E_INVALIDARG`
    fn outside(&self, object: &impl Interface) -> Result<Object> {
        let key = identity(object);
        let unknown: IUnknown = object.cast()?;
        let mut guard = self.log();
        let log = &mut *guard;
        match log.known.get(&key) {
            Some(Object::Ours(_)) => Err(invalid()),
            Some(what) => Ok(*what),
            None => {
                log.next += 1;
                let what = Object::Foreign(log.next);
                log.known.insert(key, what);
                log.held.insert(log.next, unknown);
                Ok(what)
            }
        }
    }

    /// A value: a value that isn't a number fails with `E_INVALIDARG` (the stand-in's own rule:
    /// one on the screen is a bug either way)
    fn set(&self, id: Id, property: &'static str, value: f32) -> Result<()> {
        if !value.is_finite() {
            return Err(invalid());
        }
        let mut log = self.log();
        log.live.props.insert((id, property), Prop::Value(value));
        log.live.started.remove(&(id, property));
        log.fresh.remove(&(id, property));
        log.calls.push(Call::Set {
            id,
            property,
            value,
        });
        Ok(())
    }

    /// An animation, copied as it is now: none, or one the recorder didn't make, fails with
    /// `E_INVALIDARG`
    fn animate(
        &self,
        id: Id,
        property: &'static str,
        animation: Option<&IDCompositionAnimation>,
    ) -> Result<()> {
        let animation = self.ours(animation.ok_or_else(invalid)?, &["animation"])?;
        let mut guard = self.log();
        let log = &mut *guard;
        let building = log.animations.get_mut(&animation).ok_or_else(invalid)?;
        building.handed = true;
        let curve = Curve {
            animation,
            segments: building.segments.clone(),
            end: building.end,
        };
        log.live
            .props
            .insert((id, property), Prop::Animation(curve.clone()));
        log.fresh.insert((id, property));
        log.calls.push(Call::Animated(Handed {
            id,
            property,
            curve,
        }));
        Ok(())
    }

    fn link(&self, id: Id, property: &'static str, object: Option<Object>) -> Result<()> {
        let mut log = self.log();
        match object {
            Some(object) => log.live.links.insert((id, property), object),
            None => log.live.links.remove(&(id, property)),
        };
        log.calls.push(Call::Linked {
            id,
            property,
            object,
        });
        Ok(())
    }

    /// `AddVisual` as DirectComposition does it: with no reference, `above` puts the child at
    /// the bottom and not `above` on top; with one, just above or below it. A child that has a
    /// parent or is a target's root, one that would be its own ancestor, and a reference that
    /// isn't a child fail with `E_INVALIDARG`
    fn add(
        &self,
        parent: Id,
        child: Option<&IDCompositionVisual>,
        above: bool,
        reference: Option<&IDCompositionVisual>,
    ) -> Result<()> {
        let child = Object::Ours(self.ours(child.ok_or_else(invalid)?, &["visual"])?);
        let reference = reference
            .map(|r| self.ours(r, &["visual"]))
            .transpose()?
            .map(Object::Ours);
        let mut guard = self.log();
        let log = &mut *guard;
        let tree = &mut log.live;
        if tree.parent.contains_key(&child)
            || tree.is_root(child)
            || tree.lineage(Object::Ours(parent)).contains(&child)
        {
            return Err(invalid());
        }
        let children = tree.children.entry(parent).or_default();
        let at = match reference {
            None if above => 0,
            None => children.len(),
            Some(r) => match children.iter().position(|c| *c == r) {
                Some(k) if above => k + 1,
                Some(k) => k,
                None => return Err(invalid()),
            },
        };
        children.insert(at, child);
        tree.parent.insert(child, parent);
        log.calls.push(Call::Add {
            parent,
            child,
            above,
            reference,
        });
        Ok(())
    }

    fn remove(&self, parent: Id, child: Option<&IDCompositionVisual>) -> Result<()> {
        let child = Object::Ours(self.ours(child.ok_or_else(invalid)?, &["visual"])?);
        let mut guard = self.log();
        let log = &mut *guard;
        if log.live.parent.get(&child) != Some(&parent) {
            return Err(invalid());
        }
        log.live.parent.remove(&child);
        if let Some(children) = log.live.children.get_mut(&parent) {
            children.retain(|c| *c != child);
        }
        log.calls.push(Call::Remove { parent, child });
        Ok(())
    }

    /// `SetRoot`: a visual with a parent, or another target's root, fails with `E_INVALIDARG`
    fn root(&self, target: Id, visual: Option<&IDCompositionVisual>) -> Result<()> {
        let visual = visual
            .map(|v| self.ours(v, &["visual"]))
            .transpose()?
            .map(Object::Ours);
        if let Some(visual) = visual {
            let log = self.log();
            let elsewhere = log
                .live
                .roots()
                .iter()
                .any(|(other, root)| *other != target && *root == visual);
            if log.live.parent.contains_key(&visual) || elsewhere {
                return Err(invalid());
            }
        }
        self.link(target, "Root", visual)
    }

    /// A segment on an animation: one that begins before the last, isn't a number, is a repeat
    /// with nothing before it or no length, or comes after the end fails with `E_INVALIDARG`
    /// (the stand-in's rules, where DirectComposition's documentation says none)
    fn segment(&self, id: Id, segment: Segment) -> Result<()> {
        let mut guard = self.log();
        let log = &mut *guard;
        let building = log.animations.get_mut(&id).ok_or_else(invalid)?;
        let begin = segment.begin();
        let fits = begin.is_finite()
            && begin >= 0.0
            && building.end.is_none()
            && building.segments.last().map_or(true, |s| begin >= s.begin())
            && match segment {
                Segment::Cubic(cubic) => cubic.coefficients.iter().all(|c| c.is_finite()),
                Segment::Repeat { duration, .. } => {
                    duration.is_finite() && duration > 0.0 && !building.segments.is_empty()
                }
            };
        if !fits {
            return Err(invalid());
        }
        building.segments.push(segment);
        let late = building.handed;
        log.calls.push(Call::Segment { id, segment, late });
        Ok(())
    }

    /// `End`: once, at or after the last segment's beginning
    fn end(&self, id: Id, at: f64, value: f32) -> Result<()> {
        let mut guard = self.log();
        let log = &mut *guard;
        let building = log.animations.get_mut(&id).ok_or_else(invalid)?;
        let fits = at.is_finite()
            && at >= 0.0
            && value.is_finite()
            && building.end.is_none()
            && building.segments.last().map_or(true, |s| at >= s.begin());
        if !fits {
            return Err(invalid());
        }
        building.end = Some((at, value));
        let late = building.handed;
        log.calls.push(Call::End {
            id,
            at,
            value,
            late,
        });
        Ok(())
    }

    fn commit(&self) -> Result<()> {
        let mut guard = self.log();
        let log = &mut *guard;
        let at = Instant::now();
        for key in std::mem::take(&mut log.fresh) {
            log.live.started.insert(key, at);
        }
        log.committed = log.live.clone();
        log.calls.push(Call::Commit { at });
        log.committed_at = log.calls.len();
        Ok(())
    }

    fn unrecorded<T>(&self, id: Id, method: &'static str) -> Result<T> {
        self.log().calls.push(Call::Unrecorded { id, method });
        Err(E_NOTIMPL.into())
    }
}

/// An object's COM identity: the pointer its IUnknown has, whichever interface asks
fn identity(object: &impl Interface) -> usize {
    object
        .cast::<IUnknown>()
        .map_or(0, |unknown| unknown.as_raw() as usize)
}

/// What every stand-in is: its number and the log it writes to. It takes itself out of the
/// log's identities as it goes
struct Me {
    id: Id,
    log: Recorder,
}

impl Drop for Me {
    fn drop(&mut self) {
        self.log.forget(self.id);
    }
}

impl Me {
    fn set(&self, property: &'static str, value: f32) -> Result<()> {
        self.log.set(self.id, property, value)
    }

    fn animate(&self, property: &'static str, animation: Option<&IDCompositionAnimation>) -> Result<()> {
        self.log.animate(self.id, property, animation)
    }

    /// One of the recorder's own of one of `kinds`, or none
    fn link<I: Interface>(&self, property: &'static str, object: Option<&I>, kinds: &[&str]) -> Result<()> {
        let object = object
            .map(|o| self.log.ours(o, kinds))
            .transpose()?
            .map(Object::Ours);
        self.log.link(self.id, property, object)
    }

    fn unrecorded<T>(&self, method: &'static str) -> Result<T> {
        self.log.unrecorded(self.id, method)
    }
}

/// A property's two overloads: the animation's and the value's (`2`)
macro_rules! property {
    ($animation:ident, $value:ident, $name:literal) => {
        fn $animation(&self, animation: Option<&IDCompositionAnimation>) -> Result<()> {
            self.me.animate($name, animation)
        }
        fn $value(&self, value: f32) -> Result<()> {
            self.me.set($name, value)
        }
    };
}

/// The stand-in device: `IDCompositionDesktopDevice` and `IDCompositionDevice3`, which both
/// inherit `IDCompositionDevice2`
#[implement(IDCompositionDesktopDevice, IDCompositionDevice3)]
struct Device {
    me: Me,
}

impl Device {
    fn make<I: Interface, J: Interface>(&self, kind: &'static str, make: impl FnOnce(Me) -> I) -> Result<J> {
        self.me.log.make(kind, make).cast()
    }
}

impl IDCompositionDevice2_Impl for Device_Impl {
    fn Commit(&self) -> Result<()> {
        self.me.log.commit()
    }
    fn WaitForCommitCompletion(&self) -> Result<()> {
        self.me.unrecorded("WaitForCommitCompletion")
    }
    fn GetFrameStatistics(&self, _statistics: *mut DCOMPOSITION_FRAME_STATISTICS) -> Result<()> {
        self.me.unrecorded("GetFrameStatistics")
    }
    fn CreateVisual(&self) -> Result<IDCompositionVisual2> {
        self.make::<IDCompositionVisual3, _>("visual", |me| Visual { me }.into())
    }
    fn CreateSurfaceFactory(&self, _device: Option<&IUnknown>) -> Result<IDCompositionSurfaceFactory> {
        self.me.unrecorded("CreateSurfaceFactory")
    }
    fn CreateSurface(
        &self,
        _width: u32,
        _height: u32,
        _format: DXGI_FORMAT,
        _alpha: DXGI_ALPHA_MODE,
    ) -> Result<IDCompositionSurface> {
        self.me.unrecorded("CreateSurface")
    }
    fn CreateVirtualSurface(
        &self,
        _width: u32,
        _height: u32,
        _format: DXGI_FORMAT,
        _alpha: DXGI_ALPHA_MODE,
    ) -> Result<IDCompositionVirtualSurface> {
        self.me.unrecorded("CreateVirtualSurface")
    }
    fn CreateTranslateTransform(&self) -> Result<IDCompositionTranslateTransform> {
        self.me.unrecorded("CreateTranslateTransform")
    }
    fn CreateScaleTransform(&self) -> Result<IDCompositionScaleTransform> {
        self.make::<IDCompositionScaleTransform, _>("scale", |me| Scale { me }.into())
    }
    fn CreateRotateTransform(&self) -> Result<IDCompositionRotateTransform> {
        self.make::<IDCompositionRotateTransform, _>("rotate", |me| Rotate { me }.into())
    }
    fn CreateSkewTransform(&self) -> Result<IDCompositionSkewTransform> {
        self.me.unrecorded("CreateSkewTransform")
    }
    fn CreateMatrixTransform(&self) -> Result<IDCompositionMatrixTransform> {
        self.me.unrecorded("CreateMatrixTransform")
    }
    fn CreateTransformGroup(
        &self,
        _transforms: *const Option<IDCompositionTransform>,
        _elements: u32,
    ) -> Result<IDCompositionTransform> {
        self.me.unrecorded("CreateTransformGroup")
    }
    fn CreateTranslateTransform3D(&self) -> Result<IDCompositionTranslateTransform3D> {
        self.me.unrecorded("CreateTranslateTransform3D")
    }
    fn CreateScaleTransform3D(&self) -> Result<IDCompositionScaleTransform3D> {
        self.me.unrecorded("CreateScaleTransform3D")
    }
    fn CreateRotateTransform3D(&self) -> Result<IDCompositionRotateTransform3D> {
        self.me.unrecorded("CreateRotateTransform3D")
    }
    fn CreateMatrixTransform3D(&self) -> Result<IDCompositionMatrixTransform3D> {
        self.me.unrecorded("CreateMatrixTransform3D")
    }
    fn CreateTransform3DGroup(
        &self,
        _transforms: *const Option<IDCompositionTransform3D>,
        _elements: u32,
    ) -> Result<IDCompositionTransform3D> {
        self.me.unrecorded("CreateTransform3DGroup")
    }
    fn CreateEffectGroup(&self) -> Result<IDCompositionEffectGroup> {
        self.me.unrecorded("CreateEffectGroup")
    }
    fn CreateRectangleClip(&self) -> Result<IDCompositionRectangleClip> {
        self.make::<IDCompositionRectangleClip, _>("clip", |me| Clip { me }.into())
    }
    fn CreateAnimation(&self) -> Result<IDCompositionAnimation> {
        self.make::<IDCompositionAnimation, _>("animation", |me| Animation { me }.into())
    }
}

impl IDCompositionDesktopDevice_Impl for Device_Impl {
    /// The window's handle is never used: the target only takes a root
    fn CreateTargetForHwnd(&self, _hwnd: HWND, _topmost: BOOL) -> Result<IDCompositionTarget> {
        self.make::<IDCompositionTarget, _>("target", |me| Target { me }.into())
    }
    fn CreateSurfaceFromHandle(&self, _handle: HANDLE) -> Result<IUnknown> {
        self.me.unrecorded("CreateSurfaceFromHandle")
    }
    fn CreateSurfaceFromHwnd(&self, _hwnd: HWND) -> Result<IUnknown> {
        self.me.unrecorded("CreateSurfaceFromHwnd")
    }
}

impl IDCompositionDevice3_Impl for Device_Impl {
    fn CreateGaussianBlurEffect(&self) -> Result<IDCompositionGaussianBlurEffect> {
        self.me.unrecorded("CreateGaussianBlurEffect")
    }
    fn CreateBrightnessEffect(&self) -> Result<IDCompositionBrightnessEffect> {
        self.me.unrecorded("CreateBrightnessEffect")
    }
    fn CreateColorMatrixEffect(&self) -> Result<IDCompositionColorMatrixEffect> {
        self.make::<IDCompositionColorMatrixEffect, _>("colour matrix", |me| ColourMatrix { me }.into())
    }
    fn CreateShadowEffect(&self) -> Result<IDCompositionShadowEffect> {
        self.me.unrecorded("CreateShadowEffect")
    }
    fn CreateHueRotationEffect(&self) -> Result<IDCompositionHueRotationEffect> {
        self.make::<IDCompositionHueRotationEffect, _>("hue rotation", |me| HueRotation { me }.into())
    }
    fn CreateSaturationEffect(&self) -> Result<IDCompositionSaturationEffect> {
        self.me.unrecorded("CreateSaturationEffect")
    }
    fn CreateTurbulenceEffect(&self) -> Result<IDCompositionTurbulenceEffect> {
        self.me.unrecorded("CreateTurbulenceEffect")
    }
    fn CreateLinearTransferEffect(&self) -> Result<IDCompositionLinearTransferEffect> {
        self.me.unrecorded("CreateLinearTransferEffect")
    }
    fn CreateTableTransferEffect(&self) -> Result<IDCompositionTableTransferEffect> {
        self.me.unrecorded("CreateTableTransferEffect")
    }
    fn CreateCompositeEffect(&self) -> Result<IDCompositionCompositeEffect> {
        self.me.unrecorded("CreateCompositeEffect")
    }
    fn CreateBlendEffect(&self) -> Result<IDCompositionBlendEffect> {
        self.me.unrecorded("CreateBlendEffect")
    }
    fn CreateArithmeticCompositeEffect(&self) -> Result<IDCompositionArithmeticCompositeEffect> {
        self.me.unrecorded("CreateArithmeticCompositeEffect")
    }
    fn CreateAffineTransform2DEffect(&self) -> Result<IDCompositionAffineTransform2DEffect> {
        self.me.unrecorded("CreateAffineTransform2DEffect")
    }
}

/// The stand-in target: its root
#[implement(IDCompositionTarget)]
struct Target {
    me: Me,
}

impl IDCompositionTarget_Impl for Target_Impl {
    fn SetRoot(&self, visual: Option<&IDCompositionVisual>) -> Result<()> {
        self.me.log.root(self.me.id, visual)
    }
}

/// The stand-in visual: IDCompositionVisual3, and so 2, Debug and the first
#[implement(IDCompositionVisual3)]
struct Visual {
    me: Me,
}

impl IDCompositionVisual_Impl for Visual_Impl {
    property!(SetOffsetX, SetOffsetX2, "OffsetX");
    property!(SetOffsetY, SetOffsetY2, "OffsetY");
    fn SetTransform(&self, transform: Option<&IDCompositionTransform>) -> Result<()> {
        self.me.link("Transform", transform, &["rotate", "scale"])
    }
    fn SetTransform2(&self, _matrix: *const Matrix3x2) -> Result<()> {
        self.me.unrecorded("SetTransform(matrix)")
    }
    fn SetTransformParent(&self, _visual: Option<&IDCompositionVisual>) -> Result<()> {
        self.me.unrecorded("SetTransformParent")
    }
    fn SetEffect(&self, effect: Option<&IDCompositionEffect>) -> Result<()> {
        // A transform is an effect too, as DirectComposition has it
        self.me.link(
            "Effect",
            effect,
            &["colour matrix", "hue rotation", "rotate", "scale"],
        )
    }
    fn SetBitmapInterpolationMode(&self, mode: DCOMPOSITION_BITMAP_INTERPOLATION_MODE) -> Result<()> {
        self.me.set("BitmapInterpolationMode", mode.0 as f32)
    }
    fn SetBorderMode(&self, _mode: DCOMPOSITION_BORDER_MODE) -> Result<()> {
        self.me.unrecorded("SetBorderMode")
    }
    fn SetClip(&self, clip: Option<&IDCompositionClip>) -> Result<()> {
        self.me.link("Clip", clip, &["clip"])
    }
    fn SetClip2(&self, _rect: *const D2D_RECT_F) -> Result<()> {
        self.me.unrecorded("SetClip(rect)")
    }
    /// What it shows: a swapchain wgpu made, never one of the recorder's own
    fn SetContent(&self, content: Option<&IUnknown>) -> Result<()> {
        let object = content.map(|c| self.me.log.outside(c)).transpose()?;
        self.me.log.link(self.me.id, "Content", object)
    }
    fn AddVisual(
        &self,
        visual: Option<&IDCompositionVisual>,
        insertabove: BOOL,
        referencevisual: Option<&IDCompositionVisual>,
    ) -> Result<()> {
        self.me
            .log
            .add(self.me.id, visual, insertabove.as_bool(), referencevisual)
    }
    fn RemoveVisual(&self, visual: Option<&IDCompositionVisual>) -> Result<()> {
        self.me.log.remove(self.me.id, visual)
    }
    fn RemoveAllVisuals(&self) -> Result<()> {
        self.me.unrecorded("RemoveAllVisuals")
    }
    fn SetCompositeMode(&self, _mode: DCOMPOSITION_COMPOSITE_MODE) -> Result<()> {
        self.me.unrecorded("SetCompositeMode")
    }
}

impl IDCompositionVisual2_Impl for Visual_Impl {
    fn SetOpacityMode(&self, _mode: DCOMPOSITION_OPACITY_MODE) -> Result<()> {
        self.me.unrecorded("SetOpacityMode")
    }
    fn SetBackFaceVisibility(&self, _visibility: DCOMPOSITION_BACKFACE_VISIBILITY) -> Result<()> {
        self.me.unrecorded("SetBackFaceVisibility")
    }
}

impl IDCompositionVisualDebug_Impl for Visual_Impl {
    fn EnableHeatMap(&self, _color: *const D2D1_COLOR_F) -> Result<()> {
        self.me.unrecorded("EnableHeatMap")
    }
    fn DisableHeatMap(&self) -> Result<()> {
        self.me.unrecorded("DisableHeatMap")
    }
    fn EnableRedrawRegions(&self) -> Result<()> {
        self.me.unrecorded("EnableRedrawRegions")
    }
    fn DisableRedrawRegions(&self) -> Result<()> {
        self.me.unrecorded("DisableRedrawRegions")
    }
}

impl IDCompositionVisual3_Impl for Visual_Impl {
    property!(SetOpacity, SetOpacity2, "Opacity");
    fn SetDepthMode(&self, _mode: DCOMPOSITION_DEPTH_MODE) -> Result<()> {
        self.me.unrecorded("SetDepthMode")
    }
    fn SetOffsetZ(&self, _animation: Option<&IDCompositionAnimation>) -> Result<()> {
        self.me.unrecorded("SetOffsetZ(animation)")
    }
    fn SetOffsetZ2(&self, _offsetz: f32) -> Result<()> {
        self.me.unrecorded("SetOffsetZ")
    }
    fn SetTransform(&self, _transform: Option<&IDCompositionTransform3D>) -> Result<()> {
        self.me.unrecorded("SetTransform(3D)")
    }
    fn SetTransform2(&self, _matrix: *const D2D_MATRIX_4X4_F) -> Result<()> {
        self.me.unrecorded("SetTransform(4x4)")
    }
    fn SetVisible(&self, _visible: BOOL) -> Result<()> {
        self.me.unrecorded("SetVisible")
    }
}

/// The stand-in animation: its cubics, repeats and end, copied to each property it's handed to
#[implement(IDCompositionAnimation)]
struct Animation {
    me: Me,
}

impl IDCompositionAnimation_Impl for Animation_Impl {
    fn Reset(&self) -> Result<()> {
        self.me.unrecorded("Reset")
    }
    fn SetAbsoluteBeginTime(&self, _begin: i64) -> Result<()> {
        self.me.unrecorded("SetAbsoluteBeginTime")
    }
    fn AddCubic(&self, begin: f64, c0: f32, c1: f32, c2: f32, c3: f32) -> Result<()> {
        let cubic = Cubic {
            begin,
            coefficients: [c0, c1, c2, c3],
        };
        self.me.log.segment(self.me.id, Segment::Cubic(cubic))
    }
    fn AddSinusoidal(&self, _begin: f64, _bias: f32, _amplitude: f32, _frequency: f32, _phase: f32) -> Result<()> {
        self.me.unrecorded("AddSinusoidal")
    }
    fn AddRepeat(&self, begin: f64, duration: f64) -> Result<()> {
        self.me
            .log
            .segment(self.me.id, Segment::Repeat { begin, duration })
    }
    fn End(&self, at: f64, value: f32) -> Result<()> {
        self.me.log.end(self.me.id, at, value)
    }
}

/// The stand-in rectangle clip: its edges and its corners' radii
#[implement(IDCompositionRectangleClip)]
struct Clip {
    me: Me,
}

impl IDCompositionClip_Impl for Clip_Impl {}

impl IDCompositionRectangleClip_Impl for Clip_Impl {
    property!(SetLeft, SetLeft2, "Left");
    property!(SetTop, SetTop2, "Top");
    property!(SetRight, SetRight2, "Right");
    property!(SetBottom, SetBottom2, "Bottom");
    property!(SetTopLeftRadiusX, SetTopLeftRadiusX2, "TopLeftRadiusX");
    property!(SetTopLeftRadiusY, SetTopLeftRadiusY2, "TopLeftRadiusY");
    property!(SetTopRightRadiusX, SetTopRightRadiusX2, "TopRightRadiusX");
    property!(SetTopRightRadiusY, SetTopRightRadiusY2, "TopRightRadiusY");
    property!(SetBottomLeftRadiusX, SetBottomLeftRadiusX2, "BottomLeftRadiusX");
    property!(SetBottomLeftRadiusY, SetBottomLeftRadiusY2, "BottomLeftRadiusY");
    property!(SetBottomRightRadiusX, SetBottomRightRadiusX2, "BottomRightRadiusX");
    property!(SetBottomRightRadiusY, SetBottomRightRadiusY2, "BottomRightRadiusY");
}

/// The stand-in colour matrix effect: its elements one by one, and whether it clamps
#[implement(IDCompositionColorMatrixEffect)]
struct ColourMatrix {
    me: Me,
}

impl IDCompositionEffect_Impl for ColourMatrix_Impl {}

impl IDCompositionFilterEffect_Impl for ColourMatrix_Impl {
    fn SetInput(&self, _index: u32, _input: Option<&IUnknown>, _flags: u32) -> Result<()> {
        self.me.unrecorded("SetInput")
    }
}

impl IDCompositionColorMatrixEffect_Impl for ColourMatrix_Impl {
    fn SetMatrix(&self, _matrix: *const D2D_MATRIX_5X4_F) -> Result<()> {
        self.me.unrecorded("SetMatrix")
    }
    fn SetMatrixElement(&self, row: i32, column: i32, animation: Option<&IDCompositionAnimation>) -> Result<()> {
        self.me.animate(element(row, column)?, animation)
    }
    fn SetMatrixElement2(&self, row: i32, column: i32, value: f32) -> Result<()> {
        self.me.set(element(row, column)?, value)
    }
    fn SetAlphaMode(&self, _mode: D2D1_COLORMATRIX_ALPHA_MODE) -> Result<()> {
        self.me.unrecorded("SetAlphaMode")
    }
    fn SetClampOutput(&self, clamp: BOOL) -> Result<()> {
        self.me.set("ClampOutput", if clamp.as_bool() { 1.0 } else { 0.0 })
    }
}

/// The stand-in hue rotation effect: its angle
#[implement(IDCompositionHueRotationEffect)]
struct HueRotation {
    me: Me,
}

impl IDCompositionEffect_Impl for HueRotation_Impl {}

impl IDCompositionFilterEffect_Impl for HueRotation_Impl {
    fn SetInput(&self, _index: u32, _input: Option<&IUnknown>, _flags: u32) -> Result<()> {
        self.me.unrecorded("SetInput")
    }
}

impl IDCompositionHueRotationEffect_Impl for HueRotation_Impl {
    property!(SetAngle, SetAngle2, "Angle");
}

/// The stand-in rotate transform: its angle and centre
#[implement(IDCompositionRotateTransform)]
struct Rotate {
    me: Me,
}

impl IDCompositionEffect_Impl for Rotate_Impl {}
impl IDCompositionTransform3D_Impl for Rotate_Impl {}
impl IDCompositionTransform_Impl for Rotate_Impl {}

impl IDCompositionRotateTransform_Impl for Rotate_Impl {
    property!(SetAngle, SetAngle2, "Angle");
    property!(SetCenterX, SetCenterX2, "CenterX");
    property!(SetCenterY, SetCenterY2, "CenterY");
}

/// The stand-in scale transform: its scales and centre
#[implement(IDCompositionScaleTransform)]
struct Scale {
    me: Me,
}

impl IDCompositionEffect_Impl for Scale_Impl {}
impl IDCompositionTransform3D_Impl for Scale_Impl {}
impl IDCompositionTransform_Impl for Scale_Impl {}

impl IDCompositionScaleTransform_Impl for Scale_Impl {
    property!(SetScaleX, SetScaleX2, "ScaleX");
    property!(SetScaleY, SetScaleY2, "ScaleY");
    property!(SetCenterX, SetCenterX2, "CenterX");
    property!(SetCenterY, SetCenterY2, "CenterY");
}

/// One D3D12 device on WARP (D3D12 on the CPU, in every Windows) for the whole test binary,
/// made the first time a test asks and never dropped. wgpu-hal loads dxgi.dll and d3d12.dll
/// itself and frees them with its instance and device, and the recorder holds each swapchain
/// a visual is given, as DirectComposition does, so a swapchain released after them called
/// into a library already gone: the spike's first run, a test that dropped its own instance,
/// ended the test binary with STATUS_ACCESS_VIOLATION (lbterm/PITFALLS.md, 293)
pub(crate) struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    /// For the tests that draw into a surface (acquire_*), none of them yet
    #[allow(dead_code)]
    pub queue: wgpu::Queue,
}

/// A test's turn with the one device ([`gpu`]), which it holds from before it makes a surface on
/// the device until after the last of them is dropped. wgpu-hal's D3D12 device has one idle fence
/// and one auto-reset event (`Device::idler`), which every `Surface::configure` and every
/// surface's drop wait on with no timeout (`wait_for_present_queue_idle`): two threads doing
/// either on the device at once can both wait on the event while one completion wakes one of
/// them, and the other waits for ever with no CPU. The tests run on threads of their own, so
/// they take turns; LBTerm makes and drops every surface on its one GUI thread, so it never
/// meets this (lbterm/PITFALLS.md, 303)
pub(crate) struct Turn {
    gpu: &'static Gpu,
    _turn: MutexGuard<'static, ()>,
}

impl std::ops::Deref for Turn {
    type Target = Gpu;

    fn deref(&self) -> &Gpu {
        self.gpu
    }
}

/// The one WARP device, with the turn to use it: the test's other surfaces wait for it
pub(crate) fn gpu() -> Turn {
    static TURN: Mutex<()> = Mutex::new(());
    // A test that failed holding its turn let it go as it unwound: the device is as it left it
    let turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Turn {
        gpu: device(),
        _turn: turn,
    }
}

fn device() -> &'static Gpu {
    static GPU: std::sync::OnceLock<Gpu> = std::sync::OnceLock::new();
    GPU.get_or_init(|| {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..Default::default()
        });
        let adapter = smol::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: true,
            compatible_surface: None,
        }))
        .expect("WARP, D3D12's software adapter");
        assert_eq!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
        let (device, queue) =
            smol::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("a D3D12 device on WARP");
        Gpu {
            instance,
            adapter,
            device,
            queue,
        }
    })
}

#[cfg(test)]
mod tests;
