// m3d-lns: warm-start LNS engine for the M3D routing challenge.
//
// Loads an instance + an incumbent legal solution, then runs randomized
// group rip-up / exact-SPT reroute moves (with equal-delay tie jitter),
// accepting non-worsening moves and tracking the best. The objective and
// legality rules implemented here mirror m3d/checker.py exactly; every
// improvement is intended to be re-verified by the repo checker.
use serde_json::Value;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ rng ----
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03)
    }
    #[inline]
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    #[inline]
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ------------------------------------------------------------- instance ----
struct Net {
    driver: u32,
    sinks: Vec<u32>,
}

struct Inst {
    early_stop: bool,
    retain_negotiated: bool,
    use_radix: bool,
    use_astar: bool,
    budget_bound: bool,
    free_dist: Vec<u32>,
    check_state: bool,
    name: String,
    w: u32,
    h: u32,
    l: u32,
    wh: u32,
    n: u32,
    ld: Vec<u32>,
    vd: u32,
    nets: Vec<Net>,
    pin_owner: Vec<i32>,
}

impl Inst {
    #[inline(always)]
    fn xyz(&self, v: u32) -> (u32, u32, u32) {
        (v % self.w, (v / self.w) % self.h, v / self.wh)
    }
    #[inline(always)]
    fn neigh(&self, v: u32, out: &mut [u32; 6], wts: &mut [u32; 6]) -> usize {
        let (x, y, z) = self.xyz(v);
        let lz = self.ld[z as usize];
        let mut k = 0usize;
        if x > 0 {
            out[k] = v - 1;
            wts[k] = lz;
            k += 1;
        }
        if x + 1 < self.w {
            out[k] = v + 1;
            wts[k] = lz;
            k += 1;
        }
        if y > 0 {
            out[k] = v - self.w;
            wts[k] = lz;
            k += 1;
        }
        if y + 1 < self.h {
            out[k] = v + self.w;
            wts[k] = lz;
            k += 1;
        }
        if z > 0 {
            out[k] = v - self.wh;
            wts[k] = self.vd;
            k += 1;
        }
        if z + 1 < self.l {
            out[k] = v + self.wh;
            wts[k] = self.vd;
            k += 1;
        }
        k
    }
}

fn vid_of(wh: u32, w: u32, x: u64, y: u64, z: u64) -> u32 {
    (z as u32) * wh + (y as u32) * w + (x as u32)
}

fn load_inst(path: &str) -> Inst {
    let raw = std::fs::read_to_string(path).expect("read instance");
    let d: Value = serde_json::from_str(&raw).expect("parse instance");
    let w = d["grid"]["width"].as_u64().unwrap() as u32;
    let h = d["grid"]["height"].as_u64().unwrap() as u32;
    let l = d["grid"]["layers"].as_u64().unwrap() as u32;
    let wh = w * h;
    let n = w * h * l;
    let ld: Vec<u32> = d["delay"]["layer_delay"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();
    let vd = d["delay"]["via_delay"].as_u64().unwrap() as u32;
    let name = d["name"].as_str().unwrap().to_string();
    let npins = d["pins"].as_array().unwrap().len();
    let mut pin_vid = vec![u32::MAX; npins];
    for p in d["pins"].as_array().unwrap() {
        let id = p["id"].as_u64().unwrap() as usize;
        pin_vid[id] = vid_of(
            wh,
            w,
            p["x"].as_u64().unwrap(),
            p["y"].as_u64().unwrap(),
            p["z"].as_u64().unwrap(),
        );
    }
    let nnets = d["nets"].as_array().unwrap().len();
    let mut nets: Vec<Net> = Vec::new();
    for _ in 0..nnets {
        nets.push(Net {
            driver: 0,
            sinks: Vec::new(),
        });
    }
    let mut pin_owner = vec![-1i32; n as usize];
    for nt in d["nets"].as_array().unwrap() {
        let id = nt["id"].as_u64().unwrap() as usize;
        let driver = pin_vid[nt["driver"].as_u64().unwrap() as usize];
        let sinks: Vec<u32> = nt["sinks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| pin_vid[s.as_u64().unwrap() as usize])
            .collect();
        pin_owner[driver as usize] = id as i32;
        for &s in &sinks {
            pin_owner[s as usize] = id as i32;
        }
        nets[id] = Net { driver, sinks };
    }
    let free_dist=free_space_table(w,h,l,&ld,vd);
    Inst {
        early_stop: true,
        retain_negotiated: false,
        use_radix: true,
        use_astar: true,
        budget_bound: false,
        free_dist,
        check_state: false,
        name,
        w,
        h,
        l,
        wh,
        n,
        ld,
        vd,
        nets,
        pin_owner,
    }
}

// -------------------------------------------------------------- solution ---
struct Solution {
    verts: Vec<Vec<u32>>,
    edges: Vec<Vec<(u32, u32)>>,
    delay: Vec<u64>,
    total: u64,
    vcount: u64,
}

impl Clone for Solution {
    fn clone(&self) -> Self {
        Solution {
            verts: self.verts.clone(),
            edges: self.edges.clone(),
            delay: self.delay.clone(),
            total: self.total,
            vcount: self.vcount,
        }
    }
}

fn canon(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

fn dbg_note(ws: &Scratch, member: usize, kg: usize, where_: &str) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static DBG_N: AtomicU64 = AtomicU64::new(0);
    if DBG_N.load(Ordering::Relaxed) < 80 && std::env::var("M3DDBG").is_ok() {
        DBG_N.fetch_add(1, Ordering::Relaxed);
        eprintln!(
            "M3DDBG {where_} member={member}/{kg} reason={} v={} e={}",
            ws.dbg, ws.vdbg, ws.edbg
        );
    }
}

fn edge_w(inst: &Inst, a: u32, b: u32) -> Option<u32> {
    let (ax, ay, az) = inst.xyz(a);
    let (bx, by, bz) = inst.xyz(b);
    let dx = (ax as i64 - bx as i64).abs();
    let dy = (ay as i64 - by as i64).abs();
    let dz = (az as i64 - bz as i64).abs();
    if dz == 0 && dx + dy == 1 {
        return Some(inst.ld[az as usize]);
    }
    if dx == 0 && dy == 0 && dz == 1 {
        return Some(inst.vd);
    }
    None
}

fn load_solution(inst: &Inst, path: &str) -> Result<Solution, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let d: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if d["format"].as_str() != Some("m3d-submission") { return Err("not an m3d-submission document".into()); }
    let nn = inst.nets.len();
    let mut verts: Vec<Vec<u32>> = vec![Vec::new(); nn];
    let mut edges: Vec<Vec<(u32, u32)>> = vec![Vec::new(); nn];
    let mut delay: Vec<u64> = vec![0; nn];
    let mut seen = vec![false; nn];
    for r in d["routes"].as_array().ok_or("routes missing")? {
        let nid = r["net"].as_u64().ok_or("net id")? as usize;
        if nid >= nn {
            return Err(format!("net {nid} out of range"));
        }
        if seen[nid] {
            return Err(format!("net {nid} duplicated"));
        }
        seen[nid] = true;
        let mut es: Vec<(u32, u32)> = Vec::new();
        let mut vs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for e in r["edges"].as_array().ok_or("edges missing")? {
            if e.as_array().map(|x|x.len()) != Some(2) { return Err("edge must have two endpoints".into()); }
            let a = &e[0];
            let b = &e[1];
            for p in [a,b] {
                if p.as_array().map(|x|x.len()) != Some(3) { return Err("vertex must have three coordinates".into()); }
                let x=p[0].as_u64().ok_or("invalid x coordinate")?;
                let y=p[1].as_u64().ok_or("invalid y coordinate")?;
                let z=p[2].as_u64().ok_or("invalid z coordinate")?;
                if x>=inst.w as u64 || y>=inst.h as u64 || z>=inst.l as u64 {
                    return Err(format!("coordinate out of bounds, net {nid}"));
                }
            }
            let ia = vid_of(
                inst.wh,
                inst.w,
                a[0].as_u64().unwrap(),
                a[1].as_u64().unwrap(),
                a[2].as_u64().unwrap(),
            );
            let ib = vid_of(
                inst.wh,
                inst.w,
                b[0].as_u64().unwrap(),
                b[1].as_u64().unwrap(),
                b[2].as_u64().unwrap(),
            );
            if ia >= inst.n || ib >= inst.n {
                return Err(format!("edge out of bounds, net {nid}"));
            }
            if edge_w(inst, ia, ib).is_none() {
                return Err(format!("illegal move {ia}-{ib}, net {nid}"));
            }
            es.push(canon(ia, ib));
            vs.insert(ia);
            vs.insert(ib);
        }
        vs.insert(inst.nets[nid].driver);
        for &s in &inst.nets[nid].sinks {
            vs.insert(s);
        }
        es.sort_unstable();
        if es.windows(2).any(|w|w[0]==w[1]) { return Err(format!("duplicate edge, net {nid}")); }
        es.dedup();
        // BFS from driver for tree structure + delays
        use std::collections::{HashMap, VecDeque};
        let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(x, y) in &es {
            adj.entry(x).or_default().push(y);
            adj.entry(y).or_default().push(x);
        }
        let mut dist: HashMap<u32, u64> = HashMap::new();
        dist.insert(inst.nets[nid].driver, 0);
        let mut q: VecDeque<u32> = VecDeque::new();
        q.push_back(inst.nets[nid].driver);
        while let Some(u) = q.pop_front() {
            let du = dist[&u];
            if let Some(ns) = adj.get(&u) {
                for &v in ns {
                    if !dist.contains_key(&v) {
                        let w = edge_w(inst, u, v).unwrap() as u64;
                        dist.insert(v, du + w);
                        q.push_back(v);
                    }
                }
            }
        }
        let mut t: u64 = 0;
        for &s in &inst.nets[nid].sinks {
            match dist.get(&s) {
                Some(&dd) => t += dd,
                None => return Err(format!("net {nid}: sink {s} disconnected")),
            }
        }
        if dist.len() != vs.len() {
            return Err(format!(
                "net {nid}: {} reachable of {} vertices (disconnected)",
                dist.len(),
                vs.len()
            ));
        }
        if es.len() + 1 != vs.len() {
            return Err(format!(
                "net {nid}: not a tree: {} edges, {} vertices",
                es.len(),
                vs.len()
            ));
        }
        for &v in &vs {
            let po = inst.pin_owner[v as usize];
            if po >= 0 && po != nid as i32 {
                return Err(format!("net {nid} passes through pin of net {po}"));
            }
        }
        let mut vv: Vec<u32> = vs.into_iter().collect();
        vv.sort_unstable();
        delay[nid] = t;
        verts[nid] = vv;
        edges[nid] = es;
    }
    let mut total = 0u64;
    for nid in 0..nn {
        if !seen[nid] {
            return Err(format!("net {nid} missing"));
        }
        total += delay[nid];
    }
    let mut owner = vec![-1i32; inst.n as usize];
    for nid in 0..nn {
        for &v in &verts[nid] {
            if owner[v as usize] != -1 {
                return Err(format!(
                    "vertex {v} shared by nets {} and {nid}",
                    owner[v as usize]
                ));
            }
            owner[v as usize] = nid as i32;
        }
    }
    let vcount: u64 = verts.iter().map(|v| v.len() as u64).sum();
    Ok(Solution {
        verts,
        edges,
        delay,
        total,
        vcount,
    })
}

fn write_sol(inst: &Inst, sol: &Solution, path: &str) -> std::io::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let f = File::create(path)?;
    let mut w = BufWriter::new(f);
    write!(
        w,
        "{{\"format\":\"m3d-submission\",\"version\":1,\"instance\":\"{}\",\"routes\":[",
        inst.name
    )?;
    for nid in 0..inst.nets.len() {
        if nid > 0 {
            write!(w, ",")?;
        }
        write!(w, "{{\"net\":{},\"edges\":[", nid)?;
        let mut first = true;
        for &(a, b) in &sol.edges[nid] {
            let (ax, ay, az) = inst.xyz(a);
            let (bx, by, bz) = inst.xyz(b);
            if !first {
                write!(w, ",")?;
            }
            first = false;
            write!(w, "[[{},{},{}],[{},{},{}]]", ax, ay, az, bx, by, bz)?;
        }
        write!(w, "]}}")?;
    }
    write!(w, "]}}\n")?;
    Ok(())
}

// ------------------------------------------------------------ worker ctx ---
const INF: u32 = u32::MAX;

// Minimum obstacle-free L1 distance to any sink, using the cheapest planar
// layer. This lower bound is consistent on every legal physical edge, even
// with arbitrary foreign routes removed from the graph.
#[inline]
fn goal_bound(inst:&Inst, goals:&[(u32,u32,u32)], v:u32, planar:u32) -> u32 {
    let (x,y,z)=inst.xyz(v);
    goals.iter().map(|&(a,b,c)| {
        let m=x.abs_diff(a)+y.abs_diff(b);
        if inst.free_dist.is_empty() {planar*m+inst.vd*z.abs_diff(c)}
        else {inst.free_dist[((z*inst.l+c)*(inst.w+inst.h-1)+m) as usize]}
    }).min().unwrap_or(0)
}

fn free_space_table(w:u32,h:u32,l:u32,ld:&[u32],vd:u32)->Vec<u32> {
    let mut table=Vec::with_capacity((l*l*(w+h-1)) as usize);
    for a in 0..l {for b in 0..l {for m in 0..w+h-1 {
        table.push((0..l).map(|k|ld[k as usize]*m+vd*(a.abs_diff(k)+b.abs_diff(k))).min().unwrap());
    }}}
    table
}

/// Monotone integer priority queue. Dijkstra only inserts keys at least as
/// large as the last popped key. Bucket zero holds the current minimum.
struct RadixHeap { buckets: [Vec<(u32,u32)>;33], last:u32, len:usize }
impl RadixHeap {
    fn new() -> Self { Self{buckets:std::array::from_fn(|_|Vec::new()),last:0,len:0} }
    fn clear(&mut self) { for b in &mut self.buckets {b.clear();} self.last=0;self.len=0; }
    fn push(&mut self, key:u32, v:u32) {
        debug_assert!(key>=self.last);
        let b=(32-(key^self.last).leading_zeros()) as usize;
        self.buckets[b].push((key,v));self.len+=1;
    }
    fn pop(&mut self) -> Option<(u32,u32)> {
        if self.len==0 {return None;}
        if self.buckets[0].is_empty() {
            let i=(1..33).find(|&i|!self.buckets[i].is_empty()).unwrap();
            self.last=self.buckets[i].iter().map(|x|x.0).min().unwrap();
            let mut pending=std::mem::take(&mut self.buckets[i]);
            for (k,v) in pending.drain(..) {
                let b=(32-(k^self.last).leading_zeros()) as usize;
                self.buckets[b].push((k,v));
            }
            self.buckets[i]=pending;
        }
        self.len-=1;self.buckets[0].pop()
    }
}

struct Scratch {
    dist: Vec<u32>,
    stamp: Vec<u32>,
    cur: u32,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
    radix: RadixHeap,
    gstamp: Vec<u32>,
    gcur: u32,
    rng: Rng,
    eset: std::collections::HashSet<u64>,
    // --- negotiated (penalized) search state ---
    pcur: u32,
    pdist: Vec<f64>,
    pstamp: Vec<u32>,
    pheap: BinaryHeap<Reverse<(u64, u32)>>,
    ncur: u32,
    pcv: Vec<u32>,
    pvst: Vec<u32>,
    phv: Vec<f64>,
    pce: std::collections::HashMap<u64, u32>,
    phe: std::collections::HashMap<u64, f64>,
    par: Vec<u32>,
    par_stamp: Vec<u32>,
    xcur: u32,
    dbg: u8,
    vdbg: u32,
    edbg: u32,
}

impl Scratch {
    fn new(n: u32, nn: usize, seed: u64) -> Self {
        Scratch {
            dist: vec![INF; n as usize],
            stamp: vec![0; n as usize],
            cur: 0,
            heap: BinaryHeap::new(),
            radix: RadixHeap::new(),
            gstamp: vec![0; nn],
            gcur: 0,
            rng: Rng::new(seed),
            eset: std::collections::HashSet::with_capacity(256),
            pcur: 0,
            pdist: vec![0.0; n as usize],
            pstamp: vec![0; n as usize],
            pheap: BinaryHeap::new(),
            ncur: 0,
            pcv: vec![0; n as usize],
            pvst: vec![0; n as usize],
            phv: vec![0.0; n as usize],
            pce: std::collections::HashMap::new(),
            phe: std::collections::HashMap::new(),
            par: vec![0; n as usize],
            par_stamp: vec![0; n as usize],
            xcur: 0,
            dbg: 0,
            vdbg: 0,
            edbg: 0,
        }
    }

    /// Exact Dijkstra. Blocking: all pins of another net; when `pins_only`
    /// is false, also all vertices owned by another net (the real graph).
    fn dijkstra(
        &mut self,
        inst: &Inst,
        owner: &[i32],
        src: u32,
        me: i32,
        pins_only: bool,
        cutoff: u32,
    ) {
        self.cur += 1;
        let st = self.cur;
        self.heap.clear();
        self.radix.clear();
        self.dist[src as usize] = 0;
        self.stamp[src as usize] = st;
        let astar=inst.use_astar && inst.early_stop && inst.nets[me as usize].sinks.len()<=4;
        let goals:Vec<_>=if astar {inst.nets[me as usize].sinks.iter().map(|&s|inst.xyz(s)).collect()} else {Vec::new()};
        let planar=*inst.ld.iter().min().unwrap();
        let first=if astar {goal_bound(inst,&goals,src,planar)} else {0};
        if inst.use_radix { self.radix.push(first,src); } else { self.heap.push(Reverse((first, src))); }
        let mut remaining = inst.nets[me as usize].sinks.len();
        let mut finish_at:Option<u32>=None;
        let mut nb = [0u32; 6];
        let mut wb = [0u32; 6];
        loop {
            let entry=if inst.use_radix { self.radix.pop() } else { self.heap.pop().map(|Reverse(p)|p) };
            let Some((key,v))=entry else {break;};
            if key > cutoff || finish_at.is_some_and(|limit|key>limit) {
                break;
            }
            let d=if astar {key-goal_bound(inst,&goals,v,planar)} else {key};
            if self.stamp[v as usize] != st || self.dist[v as usize] != d {
                continue;
            }
            // All positive-weight predecessors of a settled sink are settled.
            // Once the last sink is settled, extraction has the complete tight
            // predecessor DAG it needs; the rest of the grid cannot help.
            if inst.pin_owner[v as usize] == me
                && inst.nets[me as usize].sinks.contains(&v) {
                remaining -= 1;
                if remaining == 0 && inst.early_stop {
                    if !astar {break;}
                    // Finish the whole f<=largest_sink_distance plateau. Every
                    // tight predecessor of every sink lies in this set. Thus
                    // exact distances AND all equal-cost tree choices survive.
                    finish_at=Some(d);
                    continue;
                }
            }
            let k = inst.neigh(v, &mut nb, &mut wb);
            for i in 0..k {
                let u = nb[i];
                {
                    let p = inst.pin_owner[u as usize];
                    if p >= 0 && p != me {
                        continue;
                    }
                    if !pins_only {
                        let o = owner[u as usize];
                        if o >= 0 && o != me {
                            continue;
                        }
                    }
                }
                let nd = d + wb[i];
                let ui = u as usize;
                if self.stamp[ui] != st || nd < self.dist[ui] {
                    self.stamp[ui] = st;
                    self.dist[ui] = nd;
                    let priority=nd+if astar {goal_bound(inst,&goals,u,planar)} else {0};
                    if inst.use_radix { self.radix.push(priority,u); } else { self.heap.push(Reverse((priority,u))); }
                }
            }
        }
        // A cutoff must not expose tentative sink labels as an exact SPT.
        // Fail extraction before consuming random tie choices if any sink was
        // not settled within the permitted bound.
        if remaining != 0 {
            for &sink in &inst.nets[me as usize].sinks {self.stamp[sink as usize]=0;}
        }
    }

    /// Exact multi-goal search with a feasible upper bound B_i for each sink.
    /// h(v)=min_i(H(v,s_i)-B_i) is consistent. Any shortest path to sink i
    /// satisfies g(v)+h(v)<=d_i-B_i<=0, so pruning outside this union is safe.
    /// Shift by -h(root) for nonnegative monotone radix keys. Finish the
    /// complete tight-predecessor plateau after all goals settle.
    fn dijkstra_bounded(&mut self,inst:&Inst,owner:&[i32],nid:usize,budgets:&[u32],pins_only:bool) {
        let net=&inst.nets[nid];let src=net.driver;
        assert_eq!(budgets.len(),net.sinks.len());
        let goals:Vec<_>=net.sinks.iter().zip(budgets).map(|(&s,&b)|(inst.xyz(s),b)).collect();
        let planar=*inst.ld.iter().min().unwrap();
        let potential=|v:u32|->i64 {
            let (x,y,z)=inst.xyz(v);
            goals.iter().map(|&((a,b,c),bound)| {
                let m=x.abs_diff(a)+y.abs_diff(b);
                let d=if inst.free_dist.is_empty(){planar*m+inst.vd*z.abs_diff(c)}
                    else{inst.free_dist[((z*inst.l+c)*(inst.w+inst.h-1)+m) as usize]};
                d as i64-bound as i64
            }).min().unwrap_or(0)
        };
        let shift=-potential(src);
        self.cur+=1;let st=self.cur;self.heap.clear();self.radix.clear();
        self.dist[src as usize]=0;self.stamp[src as usize]=st;
        if shift<0 {for &s in &net.sinks{self.stamp[s as usize]=0;}return;}
        self.radix.push(0,src);
        let mut remaining=net.sinks.len();let mut finish=shift;
        let mut nb=[0;6];let mut wb=[0;6];
        while let Some((key,v))=self.radix.pop() {
            if key as i64>finish {break;}
            let d=(key as i64-potential(v)-shift) as u32;
            if self.stamp[v as usize]!=st||self.dist[v as usize]!=d {continue;}
            if inst.pin_owner[v as usize]==nid as i32&&net.sinks.contains(&v) {
                remaining-=1;
                if remaining==0 {
                    finish=net.sinks.iter().zip(budgets).map(|(&s,&b)|self.dist[s as usize] as i64-b as i64+shift).max().unwrap_or(0);
                }
            }
            let count=inst.neigh(v,&mut nb,&mut wb);
            for k in 0..count {
                let u=nb[k];let ui=u as usize;let pin=inst.pin_owner[ui];
                if pin>=0&&pin!=nid as i32 {continue;}
                if !pins_only&&owner[ui]>=0&&owner[ui]!=nid as i32 {continue;}
                let nd=d+wb[k];let f=nd as i64+potential(u);
                if f>0 {continue;}
                if self.stamp[ui]!=st||nd<self.dist[ui] {
                    self.stamp[ui]=st;self.dist[ui]=nd;
                    self.radix.push((f+shift) as u32,u);
                }
            }
        }
        if remaining!=0 || net.sinks.iter().zip(budgets).any(|(&s,&b)|self.dist[s as usize]>b) {
            for &s in &net.sinks{self.stamp[s as usize]=0;}
        }
    }

    /// Shortest-path tree to every sink from the last dijkstra call,
    /// randomizing among equal-cost tight predecessors.
    fn extract(&mut self, inst: &Inst, net: &Net) -> Option<(Vec<u32>, Vec<(u32, u32)>, u64)> {
        let st = self.cur;
        let mut verts: Vec<u32> = Vec::with_capacity(96);
        let mut edges: Vec<(u32, u32)> = Vec::with_capacity(96);
        self.eset.clear();
        let mut delay = 0u64;
        let mut nb = [0u32; 6];
        let mut wb = [0u32; 6];
        self.xcur += 1;
        let xid = self.xcur;
        verts.push(net.driver);
        for &sink in &net.sinks {
            if self.stamp[sink as usize] != st {
                self.dbg = 1;
                return None;
            }
            delay += self.dist[sink as usize] as u64;
            verts.push(sink);
            let mut cur = sink;
            let mut guard = 0u32;
            while cur != net.driver {
                guard += 1;
                if guard > 2_000_000 {
                    self.dbg = 2;
                    return None;
                }
                let pick: u32;
                if self.par_stamp[cur as usize] == xid {
                    pick = self.par[cur as usize];
                } else {
                    let k = inst.neigh(cur, &mut nb, &mut wb);
                    let mut picks: [u32; 6] = [0; 6];
                    let mut np = 0usize;
                    let dcur = self.dist[cur as usize];
                    for i in 0..k {
                        let u = nb[i];
                        if self.stamp[u as usize] != st {
                            continue;
                        }
                        let du = self.dist[u as usize];
                        if du != INF && du + wb[i] == dcur {
                            picks[np] = u;
                            np += 1;
                        }
                    }
                    if np == 0 {
                        self.dbg = 3;
                        return None;
                    }
                    pick = picks[self.rng.below(np as u64) as usize];
                    self.par[cur as usize] = pick;
                    self.par_stamp[cur as usize] = xid;
                }
                let ce = canon(pick, cur);
                let key = (ce.0 as u64) << 32 | ce.1 as u64;
                if self.eset.insert(key) {
                    edges.push(ce);
                }
                verts.push(pick);
                cur = pick;
            }
        }
        verts.sort_unstable();
        verts.dedup();
        if edges.len() + 1 != verts.len() {
            self.dbg = 4;
            self.vdbg = verts.len() as u32;
            self.edbg = edges.len() as u32;
            return None;
        }
        Some((verts, edges, delay))
    }

    /// Penalized (negotiated) Dijkstra with f64 costs. Vertices occupied by
    /// nets in `ing` (the negotiation group) are soft â€” penalized instead of
    /// hard-blocked. Foreign vertices and foreign pins stay hard obstacles.
    fn pdijkstra(
        &mut self,
        inst: &Inst,
        owner: &[i32],
        ing: &[bool],
        me: i32,
        src: u32,
        pres_fac: f64,
        vcong: f64,
    ) {
        self.pcur += 1;
        let st = self.pcur;
        let ng = self.ncur;
        self.pheap.clear();
        self.pdist[src as usize] = 0.0;
        self.pstamp[src as usize] = st;
        self.pheap.push(Reverse((0u64, src)));
        let mut remaining = inst.nets[me as usize].sinks.len();
        let mut nb = [0u32; 6];
        let mut wb = [0u32; 6];
        while let Some(Reverse((kb, v))) = self.pheap.pop() {
            let d = f64::from_bits(kb);
            if self.pstamp[v as usize] != st || self.pdist[v as usize] != d {
                continue;
            }
            if inst.early_stop && inst.pin_owner[v as usize] == me
                && inst.nets[me as usize].sinks.contains(&v) {
                remaining -= 1;
                if remaining == 0 { break; }
            }
            let k = inst.neigh(v, &mut nb, &mut wb);
            for i in 0..k {
                let u = nb[i];
                let p = inst.pin_owner[u as usize];
                if p >= 0 && p != me {
                    continue;
                }
                let o = owner[u as usize];
                if o >= 0 && o != me && !ing[o as usize] {
                    continue;
                }
                let vcost = if self.pvst[u as usize] == ng {
                    self.phv[u as usize] + pres_fac * self.pcv[u as usize] as f64
                } else {
                    0.0
                };
                let ek = ekey(v, u);
                let he = self.phe.get(&ek).copied().unwrap_or(0.0);
                let pe = self.pce.get(&ek).copied().unwrap_or(0) as f64;
                let nd = d + (wb[i] as f64) * (1.0 + he + pres_fac * pe) + vcong * vcost;
                let ui = u as usize;
                if self.pstamp[ui] != st || nd < self.pdist[ui] {
                    self.pstamp[ui] = st;
                    self.pdist[ui] = nd;
                    self.pheap.push(Reverse((nd.to_bits(), u)));
                }
            }
        }
    }

    /// Tree extraction for the penalized search, mirroring `extract` but with
    /// f64 distances and identical cost arithmetic (so tight edges compare
    /// bit-exactly). Random tie-breaking among equally tight predecessors.
    fn extract_pen(
        &mut self,
        inst: &Inst,
        net: &Net,
        pres_fac: f64,
        vcong: f64,
    ) -> Option<(Vec<u32>, Vec<(u32, u32)>)> {
        let st = self.pcur;
        let ng = self.ncur;
        let mut verts: Vec<u32> = Vec::with_capacity(96);
        let mut edges: Vec<(u32, u32)> = Vec::with_capacity(96);
        self.eset.clear();
        let mut nb = [0u32; 6];
        let mut wb = [0u32; 6];
        self.xcur += 1;
        let xid = self.xcur;
        verts.push(net.driver);
        for &sink in &net.sinks {
            if self.pstamp[sink as usize] != st {
                self.dbg = 5;
                return None;
            }
            verts.push(sink);
            let mut cur = sink;
            let mut guard = 0u32;
            while cur != net.driver {
                guard += 1;
                if guard > 2_000_000 {
                    self.dbg = 6;
                    return None;
                }
                let pick: u32;
                if self.par_stamp[cur as usize] == xid {
                    pick = self.par[cur as usize];
                } else {
                    let k = inst.neigh(cur, &mut nb, &mut wb);
                    let mut picks: [u32; 6] = [0; 6];
                    let mut np = 0usize;
                    let dcur = self.pdist[cur as usize];
                    let vcost_cur = if self.pvst[cur as usize] == ng {
                        self.phv[cur as usize] + pres_fac * self.pcv[cur as usize] as f64
                    } else {
                        0.0
                    };
                    for i in 0..k {
                        let u = nb[i];
                        if self.pstamp[u as usize] != st {
                            continue;
                        }
                        let ek = ekey(u, cur);
                        let he = self.phe.get(&ek).copied().unwrap_or(0.0);
                        let pe = self.pce.get(&ek).copied().unwrap_or(0) as f64;
                        let ec = (wb[i] as f64) * (1.0 + he + pres_fac * pe);
                        if self.pdist[u as usize] + ec + vcong * vcost_cur == dcur {
                            picks[np] = u;
                            np += 1;
                        }
                    }
                    if np == 0 {
                        self.dbg = 7;
                        return None;
                    }
                    pick = picks[self.rng.below(np as u64) as usize];
                    self.par[cur as usize] = pick;
                    self.par_stamp[cur as usize] = xid;
                }
                let ce = canon(pick, cur);
                let key = (ce.0 as u64) << 32 | ce.1 as u64;
                if self.eset.insert(key) {
                    edges.push(ce);
                }
                verts.push(pick);
                cur = pick;
            }
        }
        verts.sort_unstable();
        verts.dedup();
        if edges.len() + 1 != verts.len() {
            self.dbg = 8;
            self.vdbg = verts.len() as u32;
            self.edbg = edges.len() as u32;
            return None;
        }
        Some((verts, edges))
    }
}

#[inline(always)]
fn ekey(a: u32, b: u32) -> u64 {
    let (x, y) = if a < b { (a, b) } else { (b, a) };
    ((x as u64) << 32) | y as u64
}

// Evaluate the physical tree objective, independent of negotiated edge prices.
fn physical_sink_delays(inst: &Inst, nid: usize, edges: &[(u32,u32)]) -> Vec<u32> {
    let mut adj = std::collections::HashMap::<u32,Vec<u32>>::new();
    for &(a,b) in edges { adj.entry(a).or_default().push(b); adj.entry(b).or_default().push(a); }
    let mut dist = std::collections::HashMap::<u32,u64>::new();
    let root = inst.nets[nid].driver;
    dist.insert(root,0);
    let mut stack = vec![root];
    while let Some(v) = stack.pop() {
        let d = dist[&v];
        if let Some(nb) = adj.get(&v) {
            for &u in nb {
                if !dist.contains_key(&u) {
                    dist.insert(u,d+edge_w(inst,v,u).expect("tree edge") as u64);
                    stack.push(u);
                }
            }
        }
    }
    inst.nets[nid].sinks.iter().map(|s|u32::try_from(dist[s]).expect("sink delay exceeds u32")).collect()
}
fn physical_tree_delay(inst: &Inst, nid: usize, edges: &[(u32,u32)]) -> u64 {
    physical_sink_delays(inst,nid,edges).iter().map(|&d|d as u64).sum()
}

// ---------------------------------------------------------------- moves ----
fn commit(owner: &mut [i32], nid: usize, verts: &[u32]) {
    for &v in verts {
        owner[v as usize] = nid as i32;
    }
}

fn uncommit(owner: &mut [i32], verts: &[u32]) {
    for &v in verts {
        owner[v as usize] = -1;
    }
}

struct Saved {
    nid: usize,
    verts: Vec<u32>,
    edges: Vec<(u32, u32)>,
    delay: u64,
}

fn group_move(
    inst: &Inst,
    sol: &mut Solution,
    owner: &mut [i32],
    ws: &mut Scratch,
    group: &[usize],
    passes: u32,
    thr: i64,
    compact: bool,
    ub: u32,
    rescue: bool,
) -> Option<(i64, bool)> {
    if group.len() < 2 {
        return None;
    }
    let mut saved: Vec<Saved> = Vec::with_capacity(group.len());
    let mut cur_verts: Vec<Vec<u32>> = Vec::with_capacity(group.len());
    for &nid in group {
        let sv = Saved {
            nid,
            verts: sol.verts[nid].clone(),
            edges: sol.edges[nid].clone(),
            delay: sol.delay[nid],
        };
        uncommit(owner, &sv.verts);
        saved.push(sv);
        cur_verts.push(Vec::new());
    }
    let mut new_delays: Vec<u64> = vec![0; group.len()];
    let mut new_edges: Vec<Vec<(u32, u32)>> = vec![Vec::new(); group.len()];
    let mut order: Vec<usize> = (0..group.len()).collect();
    let mut ok = true;
    for _pass in 0..passes.max(1) {
        for i in (1..order.len()).rev() {
            let j = if rescue && _pass==0 {1+ws.rng.below(i as u64) as usize}
                    else {ws.rng.below((i + 1) as u64) as usize};
            order.swap(i, j);
        }
        for &gi in &order {
            if rescue && _pass==0 && gi!=0 && !cur_verts[gi].is_empty() {continue;}
            let nid = group[gi];
            uncommit(owner, &cur_verts[gi]);
            let cut = saved[gi].delay.saturating_add(ub as u64).min(u32::MAX as u64) as u32;
            ws.dijkstra(inst, owner, inst.nets[nid].driver, nid as i32, false, cut);
            match ws.extract(inst, &inst.nets[nid]) {
                Some((v, e, d)) => {
                    commit(owner, nid, &v);
                    cur_verts[gi] = v;
                    new_delays[gi] = d;
                    new_edges[gi] = e;
                    if rescue && _pass==0 && gi==0 {
                        // Keep every untouched route that still fits after the
                        // delayed seed moves. Only its actual displaced nets
                        // need repair; unrelated neutral geometry is retained.
                        for j in 1..saved.len() {
                            if saved[j].verts.iter().all(|&v|owner[v as usize]<0) {
                                commit(owner,saved[j].nid,&saved[j].verts);
                                cur_verts[j]=saved[j].verts.clone();
                                new_edges[j]=saved[j].edges.clone();
                                new_delays[j]=saved[j].delay;
                            }
                        }
                    }
                }
                None => {
                    dbg_note(ws, gi, group.len(), "group");
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            break;
        }
    }
    if ok {
        let old_sum: i64 = saved.iter().map(|s| s.delay as i64).sum();
        let new_sum: i64 = new_delays.iter().map(|&d| d as i64).sum();
        let delta = new_sum - old_sum;
        let old_v: i64 = saved.iter().map(|s| s.verts.len() as i64).sum();
        let new_v: i64 = cur_verts.iter().map(|v| v.len() as i64).sum();
        let vdelta = new_v - old_v;
        if delta <= thr && (delta != 0 || !compact || vdelta <= 0) {
            for (gi, s) in saved.iter().enumerate() {
                sol.verts[s.nid] = std::mem::take(&mut cur_verts[gi]);
                sol.edges[s.nid] = std::mem::take(&mut new_edges[gi]);
                sol.delay[s.nid] = new_delays[gi];
            }
            sol.total = (sol.total as i64 + delta) as u64;
            sol.vcount = (sol.vcount as i64 + vdelta) as u64;
            Some((delta, true))
        } else {
            for gi in 0..saved.len() {
                uncommit(owner, &cur_verts[gi]);
            }
            for s in saved.iter() {
                commit(owner, s.nid, &s.verts);
                sol.verts[s.nid] = s.verts.clone();
                sol.edges[s.nid] = s.edges.clone();
                sol.delay[s.nid] = s.delay;
            }
            Some((delta, false))
        }
    } else {
        for gi in 0..saved.len() {
            uncommit(owner, &cur_verts[gi]);
        }
        for s in saved.iter() {
            commit(owner, s.nid, &s.verts);
        }
        None
    }
}

/// Warm negotiated-congestion group move (PathFinder-style). Rip the group,
/// run penalized rounds where members may transiently overuse resources
/// (present + history costs, escalating pres_fac), then legalize with an
/// exact strict reroute pass. This mirrors the mechanism PR#5 credits for
/// its hard-tier improvements, group-scoped and warm-started from the
/// incumbent. Returns (delta, accepted); None if inapplicable/aborted.
#[allow(clippy::too_many_arguments)]
fn neg_group_move(
    inst: &Inst,
    sol: &mut Solution,
    owner: &mut [i32],
    ing: &mut [bool],
    ws: &mut Scratch,
    group: &[usize],
    rounds: u32,
    pres_fac0: f64,
    pres_mult: f64,
    hist_fac: f64,
    vcong: f64,
    thr: i64,
    compact: bool,
    ub: u32,
) -> Option<(i64, bool)> {
    if group.len() < 2 {
        return None;
    }
    let kg = group.len();
    let mut saved: Vec<Saved> = Vec::with_capacity(kg);
    for &nid in group {
        let sv = Saved {
            nid,
            verts: sol.verts[nid].clone(),
            edges: sol.edges[nid].clone(),
            delay: sol.delay[nid],
        };
        uncommit(owner, &sv.verts);
        saved.push(sv);
    }
    for &nid in group {
        ing[nid] = true;
    }
    ws.ncur += 1;
    let ng = ws.ncur;
    ws.pce.clear();
    ws.phe.clear();

    let mut cur_verts: Vec<Vec<u32>> = vec![Vec::new(); kg];
    let mut cur_edges: Vec<Vec<(u32, u32)>> = vec![Vec::new(); kg];
    let mut placed: Vec<bool> = vec![false; kg];
    let mut failed = false;

    'rounds: for r in 0..rounds.max(1) {
        let pres_fac = pres_fac0 * pres_mult.powi(r as i32);
        let mut order: Vec<usize> = (0..kg).collect();
        for i in (1..order.len()).rev() {
            let j = ws.rng.below((i + 1) as u64) as usize;
            order.swap(i, j);
        }
        for &gi in &order {
            if placed[gi] {
                for &v in &cur_verts[gi] {
                    if ws.pvst[v as usize] == ng {
                        ws.pcv[v as usize] -= 1;
                    }
                    owner[v as usize] = -1;
                }
                for &(a, b) in &cur_edges[gi] {
                    if let Some(c) = ws.pce.get_mut(&ekey(a, b)) {
                        *c = c.saturating_sub(1);
                    }
                }
            }
            let nid = group[gi];
            ws.pdijkstra(
                inst,
                owner,
                ing,
                nid as i32,
                inst.nets[nid].driver,
                pres_fac,
                vcong,
            );
            match ws.extract_pen(inst, &inst.nets[nid], pres_fac, vcong) {
                Some((v, e)) => {
                    for &vv in &v {
                        if ws.pvst[vv as usize] != ng {
                            ws.pvst[vv as usize] = ng;
                            ws.pcv[vv as usize] = 0;
                            ws.phv[vv as usize] = 0.0;
                        }
                        ws.pcv[vv as usize] += 1;
                        owner[vv as usize] = nid as i32;
                    }
                    for &(a, b) in &e {
                        *ws.pce.entry(ekey(a, b)).or_insert(0) += 1;
                    }
                    cur_verts[gi] = v;
                    cur_edges[gi] = e;
                    placed[gi] = true;
                }
                None => {
                    dbg_note(ws, gi, kg, "neg");
                    failed = true;
                    break 'rounds;
                }
            }
        }
        // overuse check + history update for the next round
        let mut over = false;
        for gi in 0..kg {
            for &v in &cur_verts[gi] {
                if ws.pvst[v as usize] == ng && ws.pcv[v as usize] >= 2 {
                    over = true;
                    ws.phv[v as usize] += hist_fac * (ws.pcv[v as usize] - 1) as f64;
                }
            }
            for &(a, b) in &cur_edges[gi] {
                if let Some(&c) = ws.pce.get(&ekey(a, b)) {
                    if c >= 2 {
                        over = true;
                        *ws.phe.entry(ekey(a, b)).or_insert(0.0) += hist_fac * (c - 1) as f64;
                    }
                }
            }
        }
        if !over {
            break;
        }
    }

    for &nid in group {
        ing[nid] = false;
    }
    ws.pce.clear();
    ws.phe.clear();

    // Preserve a conflict-free negotiated tree instead of always discarding
    // its geometry before the strict reroute. Capacity is per vertex.
    if inst.retain_negotiated && !failed && placed.iter().all(|&p| p)
        && cur_verts.iter().all(|vs| vs.iter().all(|&v| ws.pcv[v as usize] == 1)) {
        let new_delays: Vec<u64> = group.iter().enumerate()
            .map(|(gi,&nid)| physical_tree_delay(inst,nid,&cur_edges[gi])).collect();
        let old_sum: u64 = saved.iter().map(|s|s.delay).sum();
        let new_sum: u64 = new_delays.iter().sum();
        let old_v: usize = saved.iter().map(|s|s.verts.len()).sum();
        let new_v: usize = cur_verts.iter().map(|v|v.len()).sum();
        let delta = new_sum as i64 - old_sum as i64;
        if delta <= thr && (delta != 0 || !compact || new_v <= old_v) {
            for (gi,&nid) in group.iter().enumerate() {
                sol.verts[nid] = std::mem::take(&mut cur_verts[gi]);
                sol.edges[nid] = std::mem::take(&mut cur_edges[gi]);
                sol.delay[nid] = new_delays[gi];
                // During soft overlap, another member's rip-up can clear this
                // vertex's owner even after its eventual sole owner was routed.
                // Rebuild group ownership before returning a retained state.
                commit(owner,nid,&sol.verts[nid]);
            }
            sol.total = (sol.total as i64 + delta) as u64;
            sol.vcount = (sol.vcount as i64 + new_v as i64 - old_v as i64) as u64;
            return Some((delta,true));
        }
    }

    if failed {
        for gi in 0..kg {
            for &v in &cur_verts[gi] {
                owner[v as usize] = -1;
            }
        }
        for s in saved.iter() {
            commit(owner, s.nid, &s.verts);
        }
        return None;
    }

    // Blanket-free every member's negotiated tree, then exact strict polish.
    // Members may wall each other while legalizing, so retry with reshuffled
    // orders before giving up on the trial.
    let mut new_verts: Vec<Vec<u32>> = vec![Vec::new(); kg];
    let mut new_edges: Vec<Vec<(u32, u32)>> = vec![Vec::new(); kg];
    let mut new_delays: Vec<u64> = vec![0; kg];
    let mut ok = false;
    for _attempt in 0..3 {
        for gi in 0..kg {
            if placed[gi] {
                for &v in &cur_verts[gi] {
                    owner[v as usize] = -1;
                }
            }
        }
        for gi in 0..kg {
            for &v in &new_verts[gi] {
                owner[v as usize] = -1;
            }
            new_verts[gi].clear();
            new_edges[gi].clear();
        }
        let mut order: Vec<usize> = (0..kg).collect();
        for i in (1..order.len()).rev() {
            let j = ws.rng.below((i + 1) as u64) as usize;
            order.swap(i, j);
        }
        ok = true;
        for &gi in &order {
            let nid = group[gi];
            let cut = saved[gi].delay.saturating_add(ub as u64).min(u32::MAX as u64) as u32;
            ws.dijkstra(inst, owner, inst.nets[nid].driver, nid as i32, false, cut);
            match ws.extract(inst, &inst.nets[nid]) {
                Some((v, e, d)) => {
                    commit(owner, nid, &v);
                    new_verts[gi] = v;
                    new_edges[gi] = e;
                    new_delays[gi] = d;
                }
                None => {
                    dbg_note(ws, gi, kg, "polish");
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            break;
        }
    }
    if !ok {
        for gi in 0..kg {
            for &v in &new_verts[gi] {
                owner[v as usize] = -1;
            }
        }
        for s in saved.iter() {
            commit(owner, s.nid, &s.verts);
        }
        return None;
    }
    let old_sum: i64 = saved.iter().map(|s| s.delay as i64).sum();
    let new_sum: i64 = new_delays.iter().map(|&d| d as i64).sum();
    let delta = new_sum - old_sum;
    let old_v: i64 = saved.iter().map(|s| s.verts.len() as i64).sum();
    let new_v: i64 = new_verts.iter().map(|v| v.len() as i64).sum();
    let vdelta = new_v - old_v;
    if delta <= thr && (delta != 0 || !compact || vdelta <= 0) {
        for (gi, s) in saved.iter().enumerate() {
            sol.verts[s.nid] = std::mem::take(&mut new_verts[gi]);
            sol.edges[s.nid] = std::mem::take(&mut new_edges[gi]);
            sol.delay[s.nid] = new_delays[gi];
        }
        sol.total = (sol.total as i64 + delta) as u64;
        sol.vcount = (sol.vcount as i64 + vdelta) as u64;
        Some((delta, true))
    } else {
        for gi in 0..kg {
            uncommit(owner, &new_verts[gi]);
        }
        for s in saved.iter() {
            commit(owner, s.nid, &s.verts);
            sol.verts[s.nid] = s.verts.clone();
            sol.edges[s.nid] = s.edges.clone();
            sol.delay[s.nid] = s.delay;
        }
        Some((delta, false))
    }
}

fn single_move(
    inst: &Inst,
    sol: &mut Solution,
    owner: &mut [i32],
    ws: &mut Scratch,
    nid: usize,
    compact: bool,
    ub: u32,
) {
    let old_delay = sol.delay[nid];
    let old_verts = std::mem::take(&mut sol.verts[nid]);
    let old_v = old_verts.len();
    uncommit(owner, &old_verts);
    let cut = old_delay.saturating_add(ub as u64).min(u32::MAX as u64) as u32;
    if inst.budget_bound {
        let budgets=physical_sink_delays(inst,nid,&sol.edges[nid]);
        ws.dijkstra_bounded(inst,owner,nid,&budgets,false);
    }else{ws.dijkstra(inst, owner, inst.nets[nid].driver, nid as i32, false, cut);}
    match ws.extract(inst, &inst.nets[nid]) {
        Some((v, e, d)) => {
            if compact && d == old_delay && v.len() > old_v {
                commit(owner, nid, &old_verts);
                sol.verts[nid] = old_verts;
            } else {
                commit(owner, nid, &v);
                sol.delay[nid] = d;
                sol.total = (sol.total as i64 + d as i64 - old_delay as i64) as u64;
                sol.vcount = (sol.vcount as i64 + v.len() as i64 - old_v as i64) as u64;
                sol.verts[nid] = v;
                sol.edges[nid] = e;
            }
        }
        None => {
            commit(owner, nid, &old_verts);
            sol.verts[nid] = old_verts;
        }
    }
}

/// Group = seed + nets occupying a Chebyshev-box region around a random
/// vertex of the seed's route, capped at k.
fn region_group(
    inst: &Inst,
    sol: &Solution,
    owner: &[i32],
    ws: &mut Scratch,
    seed: usize,
    r: u32,
    k: usize,
) -> Vec<usize> {
    if sol.verts[seed].is_empty() || k < 2 {
        return Vec::new();
    }
    let anchor = sol.verts[seed][ws.rng.below(sol.verts[seed].len() as u64) as usize];
    let (ax, ay, _) = inst.xyz(anchor);
    let x0 = ax.saturating_sub(r);
    let x1 = (ax + r).min(inst.w - 1);
    let y0 = ay.saturating_sub(r);
    let y1 = (ay + r).min(inst.h - 1);
    ws.gcur += 1;
    let gc = ws.gcur;
    let mut members: Vec<usize> = vec![seed];
    for z in 0..inst.l {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let v = z * inst.wh + y * inst.w + x;
                let o = owner[v as usize];
                if o >= 0 && o as usize != seed && ws.gstamp[o as usize] != gc {
                    ws.gstamp[o as usize] = gc;
                    members.push(o as usize);
                }
            }
        }
    }
    if members.len() < 2 {
        return Vec::new();
    }
    if members.len() > k {
        let mut counts: Vec<(usize, u32)> = Vec::with_capacity(members.len() - 1);
        let mut cnt: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        for z in 0..inst.l {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let v = z * inst.wh + y * inst.w + x;
                    let o = owner[v as usize];
                    if o >= 0 && o as usize != seed && ws.gstamp[o as usize] == gc {
                        *cnt.entry(o as usize).or_insert(0) += 1;
                    }
                }
            }
        }
        for (nid, c) in cnt {
            counts.push((nid, c));
        }
        counts.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let take = (k - 1).min(counts.len());
        let mut out: Vec<usize> = vec![seed];
        for &(nid, _) in counts.iter().take(take) {
            out.push(nid);
        }
        return out;
    }
    members
}

/// Group = seed + owners of vertices on the seed's ideal (pins-only) tree.
fn blocker_group(
    inst: &Inst,
    sol: &Solution,
    owner: &[i32],
    ws: &mut Scratch,
    seed: usize,
    k: usize,
) -> Vec<usize> {
    if k < 2 {
        return Vec::new();
    }
    ws.dijkstra(inst, owner, inst.nets[seed].driver, seed as i32, true, u32::MAX);
    let net = &inst.nets[seed];
    let ideal = match ws.extract(inst, net) {
        Some((v, _e, _d)) => v,
        None => return Vec::new(),
    };
    ws.gcur += 1;
    let gc = ws.gcur;
    let mut members: Vec<usize> = vec![seed];
    for &v in &ideal {
        let o = owner[v as usize];
        if o >= 0 && o as usize != seed && ws.gstamp[o as usize] != gc {
            ws.gstamp[o as usize] = gc;
            members.push(o as usize);
        }
    }
    if members.len() < 2 {
        return Vec::new();
    }
    if members.len() > k {
        let mut tail: Vec<usize> = members[1..].to_vec();
        for i in (1..tail.len()).rev() {
            let j = ws.rng.below((i + 1) as u64) as usize;
            tail.swap(i, j);
        }
        let mut out = vec![seed];
        out.extend_from_slice(&tail[..k - 1]);
        return out;
    }
    members
}

// ----------------------------------------------------------------- main ----
/// Strict coordinate descent: do not discard an incumbent's neutral geometry.
/// A completed pass without changes certifies only the single-net fixed point.
fn strict_polish(inst: &Inst, sol: &mut Solution, seed: u64) {
    let mut owner = vec![-1i32; inst.n as usize];
    for nid in 0..inst.nets.len() { commit(&mut owner, nid, &sol.verts[nid]); }
    let mut ws = Scratch::new(inst.n, inst.nets.len(), seed);
    let mut passes = 0;
    let start = sol.total;
    loop {
        passes += 1;
        let before = sol.total;
        for nid in 0..inst.nets.len() {
            uncommit(&mut owner, &sol.verts[nid]);
            if inst.budget_bound {
                let budgets=physical_sink_delays(inst,nid,&sol.edges[nid]);
                ws.dijkstra_bounded(inst,&owner,nid,&budgets,false);
            }else{ws.dijkstra(inst, &owner, inst.nets[nid].driver, nid as i32, false, u32::MAX);}
            if let Some((v, e, d)) = ws.extract(inst, &inst.nets[nid]) {
                if d < sol.delay[nid] {
                    sol.total -= sol.delay[nid] - d;
                    sol.vcount = sol.vcount - sol.verts[nid].len() as u64 + v.len() as u64;
                    sol.delay[nid] = d;
                    sol.verts[nid] = v;
                    sol.edges[nid] = e;
                }
            }
            commit(&mut owner, nid, &sol.verts[nid]);
        }
        if sol.total == before { break; }
    }
    println!("POLISH passes={} before={} after={} gain={}", passes, start, sol.total, start-sol.total);
}

fn assert_consistent_state(inst:&Inst, sol:&Solution, owner:&[i32]) {
    let mut expected=vec![-1i32;inst.n as usize];
    let mut total=0;
    for nid in 0..inst.nets.len() {
        for &v in &sol.verts[nid] {
            assert_eq!(expected[v as usize],-1,"shared vertex {}",v);
            expected[v as usize]=nid as i32;
        }
        let delay=physical_tree_delay(inst,nid,&sol.edges[nid]);
        assert_eq!(delay,sol.delay[nid],"net {} delay mismatch",nid);
        total+=delay;
    }
    assert_eq!(total,sol.total);
    assert_eq!(expected,owner,"ownership cache inconsistent");
}

struct Best {
    total: u64,
    sol: Option<Solution>,
    gen: u64,
}

fn main() {
    let end_to_end_start = Instant::now();
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: m3d-lns <instance.json> <init.sol.json> <out.sol.json> [flags]");
        eprintln!("  --secs F  --seed N  --workers N  --kmin N  --kmax N  --rmax N  --passes N");
        eprintln!("  --mode region|blocker|mixed|single   --expected N  --tag STR  --verify-only");
        eprintln!("  --t0 F  --neg-frac F  --neg-rounds N  --neg-pf0 F  --neg-pm F  --neg-hf F  --neg-vc F");
        std::process::exit(2);
    }
    let inst_path = &args[1];
    let init_path = &args[2];
    let out_path = &args[3];
    let mut secs: f64 = 600.0;
    let mut seed: u64 = 12345;
    let mut workers: usize = 8;
    let mut kmin: usize = 2;
    let mut kmax: usize = 12;
    let mut rmax: u32 = 6;
    let mut passes: u32 = 2;
    let mut mode: String = "mixed".into();
    let mut expected: Option<u64> = None;
    let mut tag: String = "w".into();
    let mut verify_only = false;
    let mut full_grid = false;
    let mut retain_negotiated = false;
    let mut binary_heap = false;
    let mut dijkstra_only = false;
    let mut coarse_bound = false;
    let mut check_state = false;
    let mut max_trials = u64::MAX;
    let mut polish_only = false;
    let mut polish_final = false;
    let mut finish_walk = false;
    let mut tournament: usize = 4;
    let mut rescue = false;
    let mut distance_audit: Option<String> = None;
    let mut budget_bound = false;
    let mut walk_slack: Option<u64> = None;
    let mut dump_final = false;
    let mut nosync = false;
    let mut compact = false;
    let mut ub: u32 = u32::MAX;
    let mut t0f: f64 = 0.0;
    let mut neg_frac: f64 = 0.0;
    let mut neg_all: f64 = 0.0;
    let mut neg_rounds: u32 = 5;
    let mut neg_pf0: f64 = 0.5;
    let mut neg_pm: f64 = 1.7;
    let mut neg_hf: f64 = 0.5;
    let mut neg_vc: f64 = 1.0;
    let mut i = 4;
    while i < args.len() {
        match args[i].as_str() {
            "--full-grid" => { full_grid = true; i += 1; }
            "--retain-negotiated" => { retain_negotiated = true; i += 1; }
            "--binary-heap" => { binary_heap = true; i += 1; }
            "--dijkstra" => { dijkstra_only = true; i += 1; }
            "--coarse-bound" => { coarse_bound = true; i += 1; }
            "--check-state" => { check_state = true; i += 1; }
            "--max-trials" => { max_trials = args[i+1].parse().unwrap(); i += 2; }
            "--polish-only" => { polish_only = true; i += 1; }
            "--polish-final" => { polish_final = true; i += 1; }
            "--finish-walk" => { finish_walk = true; i += 1; }
            "--tournament" => { tournament = args[i+1].parse::<usize>().unwrap().max(1); i += 2; }
            "--rescue" => { rescue = true; i += 1; }
            "--distance-audit" => { distance_audit=Some(args[i+1].clone()); i+=2; }
            "--budget-bound" => { budget_bound=true;i+=1; }
            "--walk-slack" => {walk_slack=Some(args[i+1].parse().unwrap());i+=2;}
            "--secs" => {
                secs = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--seed" => {
                seed = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--workers" => {
                workers = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--kmin" => {
                kmin = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--kmax" => {
                kmax = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--rmax" => {
                rmax = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--passes" => {
                passes = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--mode" => {
                mode = args[i + 1].clone();
                i += 2;
            }
            "--expected" => {
                expected = Some(args[i + 1].parse().unwrap());
                i += 2;
            }
            "--tag" => {
                tag = args[i + 1].clone();
                i += 2;
            }
            "--verify-only" => {
                verify_only = true;
                i += 1;
            }
            "--dump-final" => {
                dump_final = true;
                i += 1;
            }
            "--nosync" => {
                nosync = true;
                i += 1;
            }
            "--compact" => {
                compact = true;
                i += 1;
            }
            "--ub-slack" => {
                ub = args[i + 1].parse().unwrap_or(u32::MAX);
                i += 2;
            }
            "--t0" => {
                t0f = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--neg-frac" => {
                neg_frac = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--neg-all" => {
                neg_all = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--neg-rounds" => {
                neg_rounds = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--neg-pf0" => {
                neg_pf0 = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--neg-pm" => {
                neg_pm = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--neg-hf" => {
                neg_hf = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--neg-vc" => {
                neg_vc = args[i + 1].parse().unwrap();
                i += 2;
            }
            other => {
                eprintln!("unknown flag {other}");
                std::process::exit(2);
            }
        }
    }
    let mut inst = load_inst(inst_path);
    inst.early_stop = !full_grid;
    inst.retain_negotiated = retain_negotiated;
    inst.use_radix = !binary_heap;
    inst.use_astar = !dijkstra_only;
    inst.budget_bound = budget_bound;
    if coarse_bound {inst.free_dist.clear();}
    inst.check_state = check_state;
    let inst = Arc::new(inst);
    let mut init = match load_solution(&inst, init_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("INIT ILLEGAL: {e}");
            std::process::exit(3);
        }
    };
    println!(
        "[{}] loaded {}: nets={} total={}{}",
        tag,
        inst.name,
        inst.nets.len(),
        init.total,
        match expected {
            Some(e) if e != init.total => format!("  <<< EXPECTED {e} â€” MISMATCH"),
            _ => String::new(),
        }
    );
    if verify_only {
        return;
    }
    if let Some(path)=distance_audit {
        let mut owner=vec![-1;inst.n as usize];
        for nid in 0..inst.nets.len() {commit(&mut owner,nid,&init.verts[nid]);}
        let mut ws=Scratch::new(inst.n,inst.nets.len(),seed);
        let mut rows=Vec::new();
        for nid in 0..inst.nets.len() {
            let t=Instant::now();
            if inst.budget_bound {
                let budgets=physical_sink_delays(&inst,nid,&init.edges[nid]);
                ws.dijkstra_bounded(&inst,&owner,nid,&budgets,false);
            }else{ws.dijkstra(&inst,&owner,inst.nets[nid].driver,nid as i32,false,u32::MAX);}
            let elapsed=t.elapsed().as_secs_f64();
            let distances:Vec<_>=inst.nets[nid].sinks.iter().map(|&s| {
                if ws.stamp[s as usize]==ws.cur {ws.dist[s as usize]}else{u32::MAX}
            }).collect();
            rows.push(serde_json::json!({"net":nid,"distances":distances,"seconds":elapsed,"incumbent_delay":init.delay[nid]}));
        }
        std::fs::write(path,serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
        println!("DISTANCE_AUDIT_SECONDS={:.9}",end_to_end_start.elapsed().as_secs_f64());
        return;
    }
    if polish_only {
        strict_polish(&inst, &mut init, seed);
        write_sol(&inst, &init, out_path).expect("write polished solution");
        println!("END_TO_END_SECONDS={:.9}", end_to_end_start.elapsed().as_secs_f64());
        return;
    }
    // ideal (pins-only) delays for seed weighting
    let mut scr = Scratch::new(inst.n, inst.nets.len(), seed ^ 0xABCD);
    let mut ideal_delay: Vec<u64> = vec![0; inst.nets.len()];
    {
        let owner0 = vec![-1i32; inst.n as usize];
        for nid in 0..inst.nets.len() {
            if inst.budget_bound {
                let budgets=physical_sink_delays(&inst,nid,&init.edges[nid]);
                scr.dijkstra_bounded(&inst,&owner0,nid,&budgets,true);
            }else{scr.dijkstra(&inst, &owner0, inst.nets[nid].driver, nid as i32, true, u32::MAX);}
            let mut t = 0u64;
            let mut ok = true;
            for &s in &inst.nets[nid].sinks {
                if scr.stamp[s as usize] != scr.cur {
                    ok = false;
                    break;
                }
                t += scr.dist[s as usize] as u64;
            }
            ideal_delay[nid] = if ok { t } else { u64::MAX };
        }
    }
    let shared: Arc<Mutex<Best>> = Arc::new(Mutex::new(Best {
        total: init.total,
        sol: Some(init.clone()),
        gen: 0,
    }));
    let deadline = Instant::now() + Duration::from_secs_f64(secs);
    let mut handles = Vec::new();
    for widx in 0..workers {
        let inst = Arc::clone(&inst);
        let shared = Arc::clone(&shared);
        let init2 = init.clone();
        let ideal2 = ideal_delay.clone();
        let out_path2 = out_path.to_string();
        let mode2 = if mode=="portfolio" {
            match widx%3 {0=>"single",1=>"mixed",_=>"blocker"}.to_string()
        } else {mode.clone()};
        let tournament2 = if mode=="portfolio" && widx%3==0 {1}else{tournament};
        let rescue2 = rescue || (mode=="portfolio" && widx%3==2);
        let passes2 = if mode=="portfolio" && widx%3==2 {1}else{passes};
        let tag2 = tag.clone();
        let h = std::thread::spawn(move || {
            worker(
                inst, init2, ideal2, shared, out_path2, tag2, widx, seed, mode2, kmin, kmax, rmax,
                passes2, t0f, neg_frac, neg_all, neg_rounds, neg_pf0, neg_pm, neg_hf, neg_vc,
                dump_final, nosync, compact, ub, deadline, max_trials, tournament2, finish_walk, rescue2, walk_slack,
            );
        });
        handles.push(h);
    }
    for h in handles {
        h.join().expect("search worker panicked");
    }
    let mut best = shared.lock().unwrap();
    if polish_final {
        if let Some(ref mut sol) = best.sol {
            strict_polish(&inst, sol, seed);
            best.total = sol.total;
        }
    }
    if let Some(ref sol) = best.sol {
        match write_sol(&inst, sol, out_path) {
            Ok(()) => println!(
                "[{}] FINAL total={} (init {}) written to {}",
                tag, best.total, init.total, out_path
            ),
            Err(e) => println!("[{}] FINAL total={} WRITE FAILED: {}", tag, best.total, e),
        }
    }
    println!("END_TO_END_SECONDS={:.9}", end_to_end_start.elapsed().as_secs_f64());
}

#[allow(clippy::too_many_arguments)]
fn worker(
    inst: Arc<Inst>,
    init: Solution,
    ideal_delay: Vec<u64>,
    shared: Arc<Mutex<Best>>,
    out_path: String,
    tag: String,
    widx: usize,
    seed: u64,
    mode: String,
    kmin: usize,
    kmax: usize,
    rmax: u32,
    passes: u32,
    t0: f64,
    neg_frac: f64,
    neg_all: f64,
    neg_rounds: u32,
    neg_pf0: f64,
    neg_pm: f64,
    neg_hf: f64,
    neg_vc: f64,
    dump_final: bool,
    nosync: bool,
    compact: bool,
    ub: u32,
    deadline: Instant,
    max_trials: u64,
    tournament: usize,
    finish_walk: bool,
    rescue: bool,
    walk_slack: Option<u64>,
) {
    let thr0 = t0;
    let mut sol = init.clone();
    let nn = inst.nets.len();
    let mut owner = vec![-1i32; inst.n as usize];
    for nid in 0..nn {
        commit(&mut owner, nid, &sol.verts[nid]);
    }
    let mut ing = vec![false; nn];
    let total_secs = {
        let e = deadline.saturating_duration_since(Instant::now()).as_secs_f64();
        if e > 1e-9 {
            e
        } else {
            1e-9
        }
    };
    let mut ws = Scratch::new(inst.n, nn, seed.wrapping_add((widx as u64).wrapping_mul(0x9E3779B97F4A7C15)));
    let mut trials: u64 = 0;
    let mut eq: u64 = 0;
    let mut rejected: u64 = 0;
    let mut inapp: u64 = 0;
    let mut imp: u64 = 0;
    let mut uphill: u64 = 0;
    let mut neg_trials: u64 = 0;
    let mut local_best = sol.clone();
    let mut local_best_total = sol.total;
    let mut last_report = Instant::now();
    let t0 = Instant::now();
    let mut last_sync: u64 = 0;
    let starved_n = (0..nn)
        .filter(|&nid| ideal_delay[nid] != u64::MAX && sol.delay[nid] > ideal_delay[nid])
        .count();
    let gap_sum: i64 = if ideal_delay.iter().any(|&d| d == u64::MAX) {
        -1
    } else {
        (0..nn)
            .map(|nid| (sol.delay[nid] as i64 - ideal_delay[nid] as i64).max(0))
            .sum()
    };
    println!(
        "[{}#{}] start: total={} starved={}/{} gap={} mode={} k=[{},{}] passes={}",
        tag, widx, sol.total, starved_n, nn, gap_sum, mode, kmin, kmax, passes
    );
    loop {
        if Instant::now() >= deadline || trials >= max_trials {
            break;
        }
        trials += 1;
        let mut thr: i64 = if thr0 > 0.0 {
            let rem = deadline.saturating_duration_since(Instant::now()).as_secs_f64();
            (thr0 * (rem / total_secs).clamp(0.0, 1.0)) as i64
        } else {
            0
        };
        if let Some(slack)=walk_slack {
            // Bound total excursion above the best local solution, rather
            // than accumulating one allowed uphill move after another.
            let room=local_best_total as i64+slack as i64-sol.total as i64;
            thr=thr.min(room);
        }
        // A tournament of one samples uniformly, including optimal routes
        // that may need neutral movement to release a neighboring net.
        let mut seed_net = ws.rng.below(nn as u64) as usize;
        for _ in 1..tournament {
            let cand = ws.rng.below(nn as u64) as usize;
            let ga = if ideal_delay[cand] == u64::MAX {
                i64::MIN
            } else {
                (sol.delay[cand] - ideal_delay[cand]) as i64
            };
            let gb = if ideal_delay[seed_net] == u64::MAX {
                i64::MIN
            } else {
                (sol.delay[seed_net] - ideal_delay[seed_net]) as i64
            };
            if ga > gb {
                seed_net = cand;
            }
        }
        let u: f64 = (ws.rng.next() >> 11) as f64 / ((1u64 << 53) as f64);
        let do_mode: u8 = if mode == "mixed" {
            if u < 0.75 {
                0
            } else if u < 0.90 {
                1
            } else {
                2
            }
        } else if mode == "region" {
            0
        } else if mode == "blocker" {
            1
        } else {
            2
        };
        let k = kmin + ws.rng.below((kmax - kmin + 1).max(1) as u64) as usize;
        let mut group: Vec<usize> = if do_mode == 0 {
            let r = 1 + ws.rng.below(rmax as u64) as u32;
            region_group(&inst, &sol, &owner, &mut ws, seed_net, r, k)
        } else if do_mode == 1 {
            let mut g = blocker_group(&inst, &sol, &owner, &mut ws, seed_net, k);
            if g.len() < 2 {
                let r = 1 + ws.rng.below(rmax as u64) as u32;
                g = region_group(&inst, &sol, &owner, &mut ws, seed_net, r, k);
            }
            g
        } else {
            Vec::new()
        };
        {
            let u2: f64 = (ws.rng.next() >> 11) as f64 / ((1u64 << 53) as f64);
            if do_mode != 2 && neg_all > 0.0 && u2 < neg_all {
                group = (0..nn).collect();
            }
        }
        if do_mode == 2 {
            single_move(&inst, &mut sol, &mut owner, &mut ws, seed_net, compact, ub);
        } else if group.len() < 2 {
            inapp += 1;
        } else {
            let use_neg = neg_frac > 0.0
                && ((ws.rng.next() >> 11) as f64 / ((1u64 << 53) as f64)) < neg_frac;
            let res = if use_neg {
                neg_trials += 1;
                neg_group_move(
                    &inst, &mut sol, &mut owner, &mut ing, &mut ws, &group, neg_rounds,
                    neg_pf0, neg_pm, neg_hf, neg_vc, thr, compact, ub,
                )
            } else {
                group_move(&inst, &mut sol, &mut owner, &mut ws, &group, passes, thr, compact, ub, rescue)
            };
            match res {
                Some((d, acc)) => {
                    if acc {
                        if d < 0 {
                            imp += 1;
                        } else if d == 0 {
                            eq += 1;
                        } else {
                            uphill += 1;
                        }
                    } else {
                        rejected += 1;
                    }
                }
                None => inapp += 1,
            }
        }
        if inst.check_state { assert_consistent_state(&inst,&sol,&owner); }
        if let Some(slack)=walk_slack {assert!(sol.total<=local_best_total+slack);}
        if sol.total < local_best_total {
            local_best = sol.clone();
            local_best_total = sol.total;
            imp += 1;
            let mut best = shared.lock().unwrap();
            if local_best_total < best.total {
                best.total = local_best_total;
                best.sol = Some(local_best.clone());
                best.gen += 1;
                let g = best.gen;
                if let Err(e) = write_sol(&inst, &local_best, &out_path) {
                    println!("[{}#{}] WRITE FAILED (improvement): {}", tag, widx, e);
                }
                println!(
                    "[{}#{}] NEW BEST {} (gen {}, delta vs init {}) trials={} t={:.0}s",
                    tag,
                    widx,
                    local_best_total,
                    g,
                    local_best_total as i64 - init.total as i64,
                    trials,
                    t0.elapsed().as_secs_f64()
                );
            }
        }
        let nosync_w = nosync || (widx % 2 == 1);
        if !nosync_w && trials - last_sync >= 2000 {
            last_sync = trials;
            let best = shared.lock().unwrap();
            if best.total < local_best_total {
                if let Some(ref bs) = best.sol {
                    owner.iter_mut().for_each(|x| *x = -1);
                    sol = bs.clone();
                    local_best = bs.clone();
                    local_best_total = best.total;
                    for nid in 0..nn {
                        commit(&mut owner, nid, &sol.verts[nid]);
                    }
                }
            }
        }
        if last_report.elapsed().as_secs_f64() >= 5.0 {
            last_report = Instant::now();
            println!(
                "[{}#{}] t={:.0}s trials={} rate={:.0}/s eq={} rej={} imp={} up={} neg={} inapp={} best={} (delta {})",
                tag,
                widx,
                t0.elapsed().as_secs_f64(),
                trials,
                trials as f64 / t0.elapsed().as_secs_f64().max(1e-9),
                eq,
                rejected,
                imp,
                uphill,
                neg_trials,
                inapp,
                local_best_total,
                local_best_total as i64 - init.total as i64
            );
        }
    }
    if finish_walk {
        // The best snapshot may predate thousands of useful neutral moves.
        // Complete coordinate descent on the walk's final geometry before
        // discarding it. Always compare physical delay, never search prices.
        strict_polish(&inst, &mut sol, seed.wrapping_add(widx as u64));
        if inst.check_state {
            owner.fill(-1);
            for nid in 0..nn { commit(&mut owner, nid, &sol.verts[nid]); }
            assert_consistent_state(&inst, &sol, &owner);
        }
        let mut best = shared.lock().unwrap();
        if sol.total < best.total {
            println!("FINAL_WALK before_best={} after={} gain={}", best.total, sol.total, best.total-sol.total);
            best.total = sol.total;
            best.sol = Some(sol.clone());
            best.gen += 1;
        }
    }
    if dump_final && widx == 0 {
        let _ = write_sol(&inst, &sol, &out_path);
    }
    println!(
        "[{}#{}] DONE trials={} eq={} rej={} imp={} up={} neg={} inapp={} best={} (delta {})",
        tag,
        widx,
        trials,
        eq,
        rejected,
        imp,
        uphill,
        neg_trials,
        inapp,
        local_best_total,
        local_best_total as i64 - init.total as i64
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    // Differential tests include unreachable sinks, foreign pins, blocked
    // vertices, multi-sink sharing, unequal layer weights and congestion prices.
    #[test]
    fn early_stop_preserves_physical_and_penalized_trees() {
        for seed in 1..=200 {
            let mut rng=Rng::new(seed);
            let w=4+rng.below(5) as u32; let h=4+rng.below(5) as u32; let l=1+rng.below(4) as u32;
            let n=w*h*l;
            let mut sinks=vec![n-1,n/2,n/3]; sinks.sort_unstable(); sinks.dedup();
            let mut pins=vec![-1;n as usize]; pins[0]=0;
            for &s in &sinks { pins[s as usize]=0; }
            let mut owner=vec![-1;n as usize];
            for v in 1..n { if pins[v as usize]<0 {
                if rng.below(7)==0 { owner[v as usize]=1; }
                if rng.below(19)==0 { pins[v as usize]=1; }
            }}
            let mut inst=Inst{early_stop:false,retain_negotiated:false,use_radix:false,use_astar:true,budget_bound:false,free_dist:Vec::new(),check_state:false,name:"differential".into(),w,h,l,wh:w*h,n,ld:(0..l).map(|_|1+rng.below(9) as u32).collect(),vd:1+rng.below(6) as u32,nets:vec![Net{driver:0,sinks}],pin_owner:pins};
            inst.free_dist=free_space_table(w,h,l,&inst.ld,inst.vd);
            let mut full=Scratch::new(n,1,seed); let mut fast=Scratch::new(n,1,seed);
            full.dijkstra(&inst,&owner,0,0,false,u32::MAX);
            let a=full.extract(&inst,&inst.nets[0]);
            inst.early_stop=true;
            inst.use_radix=true;
            fast.dijkstra(&inst,&owner,0,0,false,u32::MAX);
            let b=fast.extract(&inst,&inst.nets[0]);
            assert_eq!(a,b,"physical seed {}",seed);
            if a.is_some() {
                for slack in [0,7,100] {
                    let budgets:Vec<_>=inst.nets[0].sinks.iter().map(|&s|fast.dist[s as usize]+slack).collect();
                    let mut bounded=Scratch::new(n,1,seed);
                    bounded.dijkstra_bounded(&inst,&owner,0,&budgets,false);
                    assert_eq!(a,bounded.extract(&inst,&inst.nets[0]),"per-sink budget seed {} slack {}",seed,slack);
                }
            }
            for ws in [&mut full,&mut fast] { ws.ncur=1; }
            for v in 0..n as usize {
                let count=rng.below(4) as u32; let hist=rng.below(8) as f64 * 0.5;
                for ws in [&mut full,&mut fast] { ws.pvst[v]=1;ws.pcv[v]=count;ws.phv[v]=hist; }
            }
            inst.early_stop=false;
            full.pdijkstra(&inst,&owner,&[false,false],0,0,0.5,1.0);
            let a=full.extract_pen(&inst,&inst.nets[0],0.5,1.0);
            inst.early_stop=true;
            fast.pdijkstra(&inst,&owner,&[false,false],0,0,0.5,1.0);
            let b=fast.extract_pen(&inst,&inst.nets[0],0.5,1.0);
            assert_eq!(a,b,"penalized seed {}",seed);
            for cut in [2,7,14,30] {
                let mut full=Scratch::new(n,1,seed);let mut fast=Scratch::new(n,1,seed);
                inst.early_stop=false;inst.use_radix=false;
                full.dijkstra(&inst,&owner,0,0,false,cut);
                let a=full.extract(&inst,&inst.nets[0]);
                inst.early_stop=true;inst.use_radix=true;
                fast.dijkstra(&inst,&owner,0,0,false,cut);
                let b=fast.extract(&inst,&inst.nets[0]);
                assert_eq!(a,b,"bounded seed {} cutoff {}",seed,cut);
            }
        }
    }
    #[test]
    fn tree_objective_charges_shared_trunk_for_each_sink() {
        let inst=Inst{early_stop:true,retain_negotiated:true,use_radix:true,use_astar:true,budget_bound:false,free_dist:Vec::new(),check_state:false,name:"trunk".into(),w:3,h:2,l:1,wh:6,n:6,ld:vec![2],vd:3,nets:vec![Net{driver:0,sinks:vec![2,4]}],pin_owner:vec![0,-1,0,-1,0,-1]};
        assert_eq!(physical_tree_delay(&inst,0,&[(0,1),(1,2),(1,4)]),8);
    }
}
