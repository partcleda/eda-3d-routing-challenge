#!/usr/bin/env python3
"""Independent cold routing with immutable per-case producing source versions."""
import argparse, hashlib, json, os, platform, shutil, subprocess, sys, tempfile, time
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1] if HERE.parent.name==".local-validation" else HERE.parents[2]
sys.path.insert(0,str(ROOT))
from m3d.checker import check
from m3d.model import Instance,Submission
TIERS=("intro","hard","scale","stress","congested","designs")

def digest(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def instance_digest(p):
    return hashlib.sha256(Path(p).read_bytes().replace(b"\r\n",b"\n")).hexdigest()
def atomic_bytes(p,data,immutable=False):
    p=Path(p)
    with tempfile.NamedTemporaryFile(dir=p.parent,prefix='.'+p.name+'.',suffix='.tmp',delete=False) as fh:
        tmp=Path(fh.name);fh.write(data)
    try:
        for attempt in range(20):
            try:
                if immutable:
                    try:os.link(tmp,p)
                    except FileExistsError:assert p.read_bytes()==data,'immutable source changed'
                else:tmp.replace(p)
                break
            except PermissionError as error:
                if os.name!='nt' or getattr(error,'winerror',None) not in (5,32,33) or attempt==19:raise
                time.sleep(0.01)
    finally:
        for attempt in range(20):
            try:tmp.unlink(missing_ok=True);break
            except PermissionError as error:
                if os.name!='nt' or getattr(error,'winerror',None) not in (5,32,33) or attempt==19:raise
                time.sleep(0.01)

def write_json(p,data,immutable=False):
    atomic_bytes(p,(json.dumps(data,indent=2)+"\n").encode('utf-8'),immutable)
def version_id(hashes):
    return hashlib.sha256("".join(f"{k}:{hashes[k]}\n" for k in sorted(hashes)).encode()).hexdigest()
def snapshot():
    files=[HERE/"run.py",HERE/"router/Cargo.toml",HERE/"router/Cargo.lock"]+sorted((HERE/"router/src").rglob("*.rs"))
    contents={p.relative_to(HERE).as_posix():p.read_bytes() for p in files}
    hashes={name:hashlib.sha256(raw).hexdigest() for name,raw in contents.items()}
    version=version_id(hashes);base=HERE/"reproducers"/version
    for name,raw in contents.items():
        q=base/name;q.parent.mkdir(parents=True,exist_ok=True)
        if q.exists():assert q.read_bytes()==raw,"immutable source changed"
        else:atomic_bytes(q,raw,immutable=True)
    if not (base/"source.json").exists():
        write_json(base/"source.json",{"format":"cold-spt-source-v1","version":version,"files":hashes},immutable=True)
    return version,hashes
def validate_source(version,hashes):
    assert version_id(hashes)==version
    base=HERE/"reproducers"/version
    assert json.loads((base/"source.json").read_text())["files"]==hashes
    for name,value in hashes.items():assert digest(base/name)==value,(version,name)
    return base
def build(version,hashes,cargo,built):
    base=validate_source(version,hashes);target=Path(tempfile.gettempdir())/"cold-spt-build"/version
    if version not in built:
        subprocess.run([cargo,"build","--release","--locked","--manifest-path",str(base/"router/Cargo.toml"),"--target-dir",str(target)],check=True)
        built.add(version)
    return target/"release"/("cold-spt-lns.exe" if os.name=="nt" else "cold-spt-lns")

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--tier",choices=TIERS,action="append");ap.add_argument("--case")
    ap.add_argument("--out-root",type=Path);ap.add_argument("--reproduce",action="store_true")
    ap.add_argument("--check-only",action="store_true");ap.add_argument("--only-improvements",action="store_true")
    ap.add_argument("--cargo",default="cargo");ap.add_argument("--hardware",default=platform.processor() or platform.machine())
    options={"seed":20261022,"starts":1,"rounds":300,"sweeps":0,"moves":0,"construction-weight":2,"refine-trials":4000,"negotiation-rounds":0,"fanout-priority":0,"slow-congestion":0,"legalize-conflicts":0,"cost-distance":0,"refine-each-start":0,"cost-distance-rooted":0,"cost-distance-discount":0,"refine-temperature":32,"refine-slack":128,"refine-group-max":16,"compact-shortest-tree":0,"post-cbs-trials":0,"cbs-nodes":256,"emit-global-pool":0}
    options["cross-profile-plan"]=0
    options["subset-tree-max-sinks"]=0
    options["marginal-tree-price"]=0
    options["owner-set-groups"]=0
    options["joint-neutral"]=0
    options["penalized-astar"]=0
    options["retain-compact-best"]=0
    options["flow-pricing"]=0
    options["joint-group-max"]=4
    options["penalized-integer-queue"]=0
    options["penalized-heuristic-cache"]=0
    options["refine-race-pilot"]=0
    options["refine-race-keep"]=2
    options["refine-forks"]=1
    options["compact-polish-passes"]=0
    options["cbs-group-max"]=4
    options["post-cbs-bypass-trials"]=0
    options["cbs-conflict-priority"]=0
    options["final-stable-polish-passes"]=0
    options["layer-direction-preference"]=0
    for flag,value in options.items():
        if flag=="layer-direction-preference":ap.add_argument("--"+flag,type=int,choices=(0,1,2),default=value,help="0=uniform SPT; 1=fixed alternating layer direction; 2=paired per-net phase selection (two construction queries per affected net)")
        elif flag=="owner-set-groups":ap.add_argument("--"+flag,type=int,choices=(0,1,2,3),default=value,help="0=off; 1=blocker/region control; 2=owner-set sink paths; 3=detour-ranked shortcut owner-set postphase")
        else:ap.add_argument("--"+flag,type=int,default=value)
    ap.add_argument("--refine-mode",choices=("portfolio","portfolio-joint","portfolio-joint-entry","portfolio-joint-polish","portfolio-cbs","cbs","cbs-mixed","mixed","mixed-joint","joint","chain-fast","chain-entry-fast","chain-polish-fast","single"),default="portfolio")
    ap.add_argument("--tree-root-weight",type=int,choices=(0,25,50,75,100))
    a=ap.parse_args()
    if a.check_only and a.reproduce:ap.error("choose --check-only or --reproduce")
    if a.marginal_tree_price and a.out_root is None:ap.error("--marginal-tree-price requires an explicit --out-root")
    generating=not(a.check_only or a.reproduce);current=snapshot() if generating else None;built=set()
    if generating and a.emit_global_pool and a.out_root is None:ap.error("research pools require --out-root")
    settings=[]
    for flag in list(options)+["refine-mode"]:
        if flag=="marginal-tree-price" and not a.marginal_tree_price:continue
        settings += ["--"+flag,str(getattr(a,flag.replace("-","_")))]
    if a.tree_root_weight is not None:settings += ["--tree-root-weight",str(a.tree_root_weight)]
    for tier in a.tier or TIERS:
        suite=ROOT/("benchmarks" if tier=="intro" else "benchmarks_"+tier)
        cases=json.loads((suite/"suite.json").read_text())["cases"]
        selected=[c for c in cases if not a.case or c["name"]==a.case]
        if not selected:raise ValueError("case not in selected tier")
        dest=(a.out_root or ROOT/"submissions")/tier/"cold_spt_lns"
        if generating:dest.mkdir(parents=True,exist_ok=True)
        rp,sp=dest/"runtime.json",dest/"runs.json"
        times=json.loads(rp.read_text()) if rp.exists() else {}
        records=json.loads(sp.read_text()) if sp.exists() else {}
        for c in selected:
            name=c["name"];original=suite/c["instance_file"];committed=dest/(name+".sol.json");record=records.get(name)
            if not generating:
                assert record is not None,"no producing record: "+tier+"/"+name
                validate_source(record["source_version"],record["source_sha256"])
                assert record["input_sha256"]==instance_digest(original)
                assert record["output_sha256"]==digest(committed)
                assert times[name]==record["wall_seconds"]
            if a.check_only:
                result=check(Instance.load(str(original)),Submission.load(str(committed)))
                assert result.legal and result.total_delay==record["total_delay"]
                print(f"CHECK {tier}/{name}: legal, delay={result.total_delay}, archived source and hashes match",flush=True)
                continue
            version,hashes=current if generating else (record["source_version"],record["source_sha256"])
            exe=build(version,hashes,a.cargo,built);argv=settings if generating else record["solver_args"]
            logdir=ROOT/".local-validation/search-logs";logdir.mkdir(parents=True,exist_ok=True)
            logpath=logdir/f"{tier}-{name}-{time.time_ns()}.log"
            with tempfile.TemporaryDirectory(prefix="cold-spt-") as td:
                td=Path(td);instance=td/"instance.json";out=td/"output.sol.json";shutil.copyfile(original,instance)
                lines=[];started=time.perf_counter()
                with logpath.open("w",encoding="utf-8") as log:
                    process=subprocess.Popen([str(exe),str(instance),str(out)]+argv,cwd=td,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
                    print(f"START {tier}/{name}: pid={process.pid}, source={version}, log={logpath}",flush=True)
                    for line in process.stdout:
                        lines.append(line.rstrip("\n"));log.write(line);log.flush()
                        if any(t in line for t in ("cold start","construction","start ","t=","DONE","FINAL","POLISH")):
                            print(f"{tier}/{name}: {line.rstrip()}",flush=True)
                    code=process.wait()
                elapsed=time.perf_counter()-started
                if code:raise RuntimeError(f"{tier}/{name}: exit {code}, log={logpath}")
                result=check(Instance.load(str(instance)),Submission.load(str(out)))
                assert result.legal,result.to_dict()
                if generating and a.emit_global_pool:
                    pool=json.loads(Path(str(out)+".pool.json").read_text())
                    assert pool['incumbent_delay']==result.total_delay
                    pool['incumbent_output_sha256']=digest(out)
                    write_json(dest/(name+".pool.json"),pool)
                if a.reproduce:
                    assert out.read_bytes()==committed.read_bytes(),"reproduction differs"
                    print(f"REPLAY {tier}/{name}: byte-identical, delay={result.total_delay}, seconds={elapsed:.3f}",flush=True)
                    continue
                if a.only_improvements and committed.exists():
                    prev=check(Instance.load(str(original)),Submission.load(str(committed)))
                    if prev.legal and result.total_delay>=prev.total_delay:
                        print(f"KEEP {tier}/{name}: existing={prev.total_delay}, candidate={result.total_delay}, seconds={elapsed:.3f}",flush=True)
                        continue
                shutil.copyfile(out,committed);times[name]=elapsed
                records[name]={"input_sha256":instance_digest(instance),"input_file_sha256":digest(instance),
                    "input_hash_scope":"UTF-8 instance text with CRLF normalized to LF","output_sha256":digest(out),
                    "source_version":version,"source_sha256":hashes,"binary_sha256":digest(exe),
                    "solver_args":argv,"total_delay":result.total_delay,"wall_seconds":elapsed,
                    "hardware":a.hardware,"platform":platform.platform(),"threads":1,"solver_log":lines}
                write_json(rp,times);write_json(sp,records)
                print(f"ADOPT {tier}/{name}: legal, delay={result.total_delay}, full_runtime={elapsed:.3f}",flush=True)
if __name__=="__main__":main()
