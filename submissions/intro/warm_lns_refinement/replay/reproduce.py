"""Replay versioned measured pipelines or independently check archived routes.

Timed, concurrent searches can produce different legal results on a rerun.
--fixed-work reproduces the recorded number of moves for single-worker stages.
--require-identical also demands exact route-file identity at every stage.
"""
import argparse,hashlib,json,os,re,subprocess,sys,time
from pathlib import Path
BASE=Path(__file__).resolve().parent
parser=argparse.ArgumentParser()
parser.add_argument('--repo',type=Path,required=True,help='Challenge main182434d checkout')
parser.add_argument('--pr24',type=Path,help='PR24 58585d7 submissions directory')
parser.add_argument('--pr26',type=Path,help='PR26 89ca440 submissions directory')
parser.add_argument('--out',type=Path)
parser.add_argument('--tier');parser.add_argument('--case')
parser.add_argument('--check-only',action='store_true')
parser.add_argument('--fixed-work',action='store_true')
parser.add_argument('--require-identical',action='store_true')
args=parser.parse_args();sys.path.insert(0,str(args.repo.resolve()))
from m3d.model import Instance,Submission
from m3d.checker import check
manifest=json.loads((BASE/'manifest.json').read_text())
dirs={'intro':'benchmarks','hard':'benchmarks_hard','scale':'benchmarks_scale','stress':'benchmarks_stress','congested':'benchmarks_congested','designs':'benchmarks_designs'}
results=[]
replay_cache={};stage_costs={}
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def replay(row,depth=0):
    key=hashlib.sha256((row['output']+row.get('measured_at','')).encode()).hexdigest()[:16]
    if key in replay_cache:return replay_cache[key]
    tier,name=row['tier'],row['case'];ip=args.repo/dirs[tier]/(name+'.json');dependencies=set()
    out=args.out/tier/(name+f'.stage-{key}.sol.json');out.parent.mkdir(parents=True,exist_ok=True)
    if row.get('kind')=='pairwise':
        parents=[replay(parent,depth+1) for parent in row['parent_measurements']]
        for parent in parents:dependencies.update(parent['stage_keys'])
        script=BASE/'reproducer/algorithms'/row['algorithm_sha256']/'pairwise_refine.py'
        assert sha(script)==row['algorithm_sha256']
        cmd=[sys.executable,str(script),str(ip),parents[0]['output'],parents[1]['output'],str(out)]
    elif 'parent_measurement' in row:
        previous=replay(row['parent_measurement'],depth+1);src=Path(previous['output']);dependencies.update(previous['stage_keys'])
    else:
        entry=Path(row['command'][2]).parent.name
        root=args.pr24 if entry=='coordinated_refinement' else args.pr26 if entry=='drama3d-portfolio' else args.repo/'submissions'
        assert root is not None,('missing public input directory for',entry)
        src=root/tier/entry/(name+'.sol.json')
        if sha(src)!=row['source_sha256']:
            # Git for Windows may have converted a public text blob to CRLF.
            # Accept only normalization that recovers the exact recorded hash.
            canonical=src.read_bytes().replace(b'\r\n',b'\n')
            assert hashlib.sha256(canonical).hexdigest()==row['source_sha256'],('incorrect public input revision',src)
            src=args.out/'_public'/tier/entry/(name+'.sol.json')
            src.parent.mkdir(parents=True,exist_ok=True);src.write_bytes(canonical)
    if row.get('kind')!='pairwise':
        exe=BASE/'reproducer/versions'/row['exe_sha256']/'m3d-lns.exe'
        assert sha(exe)==row['exe_sha256']
        flags=list(row['command'][4:]);workers=int(flags[flags.index('--workers')+1]) if '--workers' in flags else 8
        if args.fixed_work and workers==1:
            matches=re.findall(r'\[.*?#0\] DONE trials=(\d+)',row.get('log',''))
            if matches:flags+=['--max-trials',matches[-1],'--secs','3600']
        cmd=[str(exe),str(ip),str(src),str(out),*flags]
    env=dict(os.environ);env['PYTHONPATH']=str(args.repo)+os.pathsep+env.get('PYTHONPATH','')
    started=time.perf_counter();run=subprocess.run(cmd,capture_output=True,text=True,env=env);solver_s=time.perf_counter()-started
    (out.parent/(out.stem+'.log')).write_text(run.stdout+'\n'+run.stderr,encoding='utf-8')
    assert run.returncode==0,(cmd,run.stderr)
    t=time.perf_counter();checked=check(Instance.load(str(ip)),Submission.load(str(out)));validation=time.perf_counter()-t
    assert checked.legal,(tier,name,checked.reasons)
    digest=sha(out);identical=digest==row['sha256']
    if args.require_identical:assert identical,(tier,name,depth,'rerun differs from timed selected output')
    stage_costs[key]=solver_s+validation;dependencies.add(key)
    result=dict(tier=tier,case=name,depth=depth,delay=checked.total_delay,recorded_delay=row['delay'],identical=identical,sha256=digest,output=str(out),solver_wall_s=solver_s,validation_s=validation,runtime_s=sum(stage_costs[k] for k in dependencies),stage_keys=sorted(dependencies),command=cmd)
    replay_cache[key]=result;print(json.dumps(result),flush=True);return result
for row in manifest['cases']:
    tier,name=row['tier'],row['case']
    if args.tier and args.tier!=tier:continue
    if args.case and args.case!=name:continue
    if args.check_only:
        out=args.repo/'submissions'/tier/'warm_lns_refinement'/(name+'.sol.json');assert sha(out)==row['sha256']
        checked=check(Instance.load(str(args.repo/dirs[tier]/(name+'.json'))),Submission.load(str(out)))
        assert checked.legal and checked.total_delay==row['delay']
        results.append(dict(tier=tier,case=name,legal=True,delay=checked.total_delay));print(tier,name,checked.total_delay,flush=True)
    else:
        assert args.out and args.pr24,'--out and --pr24 are required for replay'
        results.append(replay(row))
if args.out:
    args.out.mkdir(parents=True,exist_ok=True);(args.out/'reproduction_results.json').write_text(json.dumps(results,indent=2))
