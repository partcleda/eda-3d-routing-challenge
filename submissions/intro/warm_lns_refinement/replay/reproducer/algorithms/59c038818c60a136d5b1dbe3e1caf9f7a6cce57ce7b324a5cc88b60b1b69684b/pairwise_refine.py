"""Exact two-parent route crossover by maximum-weight closure.

This is a refinement operator on existing legal routes, following the method
disclosed by jay-tau in PR24. It is not a global router or multi-parent optimum.
Selecting B_i instead of A_i gains d(A_i)-d(B_i). A cross-parent conflict
B_i / A_j implies selecting B_i also selects B_j. Minimum cut solves exactly
this binary choice problem. Both all-A and all-B remain feasible.
"""
import argparse,hashlib,json,random,sys,time
from collections import deque
from pathlib import Path
P=Path(__file__).resolve().parent;R=P.parent/'audit-20261001/repo'
sys.path.insert(0,str(R));sys.setrecursionlimit(10000)
from m3d.model import Instance,Submission
from m3d.checker import check

def closure(weights,implications):
    n=len(weights);source=n;sink=n+1;adj=[[] for _ in range(n+2)]
    def edge(a,b,c):
        adj[a].append([b,c,len(adj[b])]);adj[b].append([a,0,len(adj[a])-1])
    for i,w in enumerate(weights):
        if w>0:edge(source,i,w)
        if w<0:edge(i,sink,-w)
    inf=1+sum(abs(w) for w in weights)
    for a,b in sorted(set(implications)):edge(a,b,inf)
    while True:
        level=[-1]*(n+2);level[source]=0;queue=deque([source])
        while queue:
            a=queue.popleft()
            for b,c,_ in adj[a]:
                if c>0 and level[b]<0:level[b]=level[a]+1;queue.append(b)
        if level[sink]<0:break
        pos=[0]*(n+2)
        def send(a,cap):
            if a==sink:return cap
            while pos[a]<len(adj[a]):
                e=adj[a][pos[a]];b,c,rev=e
                if c>0 and level[b]==level[a]+1:
                    f=send(b,min(cap,c))
                    if f:e[1]-=f;adj[b][rev][1]+=f;return f
                pos[a]+=1
            return 0
        while send(source,inf):pass
    reachable={source};queue=deque([source])
    while queue:
        a=queue.popleft()
        for b,c,_ in adj[a]:
            if c>0 and b not in reachable:reachable.add(b);queue.append(b)
    chosen={i for i in range(n) if i in reachable}
    assert all(a not in chosen or b in chosen for a,b in implications)
    return chosen

def combine(instance,a_path,b_path,out_path):
    t=time.perf_counter();cpu=time.process_time()
    inst=Instance.load(str(instance));a=Submission.load(str(a_path));b=Submission.load(str(b_path))
    ar=check(inst,a);br=check(inst,b);assert ar.legal and br.legal
    ra={r.net:r for r in a.routes};rb={r.net:r for r in b.routes};ids=sorted(ra);assert ids==sorted(rb)
    index={nid:i for i,nid in enumerate(ids)};da={r.net:r.delay for r in ar.nets};db={r.net:r.delay for r in br.nets}
    owner={v:index[r.net] for r in a.routes for e in r.edges for v in e};implications=set()
    for route in b.routes:
        i=index[route.net]
        for edge in route.edges:
            for vertex in edge:
                j=owner.get(vertex)
                if j is not None and j!=i:implications.add((i,j))
    weights=[da[i]-db[i] for i in ids];chosen=closure(weights,implications)
    result=Submission(instance=inst.name,routes=[rb[nid] if index[nid] in chosen else ra[nid] for nid in ids])
    out_path=Path(out_path);out_path.parent.mkdir(parents=True,exist_ok=True)
    out_path.write_text(json.dumps(result.to_dict(),separators=(',',':')),encoding='utf-8')
    # Independently reload the actual saved bytes and recompute the objective.
    checked=check(Instance.load(str(instance)),Submission.load(str(out_path)))
    predicted=ar.total_delay-sum(weights[i] for i in chosen)
    assert checked.legal and checked.total_delay==predicted<=min(ar.total_delay,br.total_delay)
    return dict(delay=predicted,legal=True,runtime_s=time.perf_counter()-t,solver_cpu_s=time.process_time()-cpu,output=str(out_path),sha256=hashlib.sha256(out_path.read_bytes()).hexdigest(),a_delay=ar.total_delay,b_delay=br.total_delay,selected_b=len(chosen),implications=len(implications))

def self_test():
    rng=random.Random(20261002)
    for trial in range(250):
        n=rng.randint(1,10);weights=[rng.randint(-15,15) for _ in range(n)];arcs={(i,j) for i in range(n) for j in range(n) if i!=j and rng.random()<0.15}
        best=max(sum(weights[i] for i in range(n) if mask>>i&1) for mask in range(1<<n) if all(not(mask>>i&1) or mask>>j&1 for i,j in arcs))
        chosen=closure(weights,arcs);assert sum(weights[i] for i in chosen)==best
    print('250 closure graphs matched exhaustive enumeration',flush=True)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('instance',nargs='?');p.add_argument('a',nargs='?');p.add_argument('b',nargs='?');p.add_argument('out',nargs='?');p.add_argument('--self-test',action='store_true');args=p.parse_args()
    if args.self_test:self_test()
    else:print(json.dumps(combine(args.instance,args.a,args.b,args.out)),flush=True)
