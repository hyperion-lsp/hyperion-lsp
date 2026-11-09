import re,glob,os,sys
# Usage: gen_kotlin_stdlib_members.py OUT.tsv [UNPACKED_STDLIB_SOURCES_DIR]
#
# Regenerates hyperion-kotlin/src/stdlib_members.tsv from an unpacked
# kotlin-stdlib sources jar (`unzip kotlin-stdlib-2.2.20-sources.jar -d DIR`).
# Declaration headers only; see hyperion-kotlin/src/stdlib_members.rs.
root=sys.argv[2] if len(sys.argv)>2 else '/tmp/kotlin-stdlib-sources'
files=[]
for sub in ('commonMain','jvmMain'):
    files+=glob.glob(f'{root}/{sub}/**/*.kt',recursive=True)
files=[f for f in files if '/internal/' not in f and 'Test' not in os.path.basename(f) and '/js/' not in f]

def strip_comments(src):
    out=[];i=0;n=len(src);in_str=None
    while i<n:
        c=src[i]
        if in_str:
            out.append(c)
            if c=='\\': out.append(src[i+1]); i+=2; continue
            if src.startswith(in_str,i) : out.append(in_str[1:]); i+=len(in_str); in_str=None; continue
            i+=1; continue
        if src.startswith('/*',i):
            j=src.find('*/',i+2); i=(j+2) if j>=0 else n; out.append(' '); continue
        if src.startswith('//',i):
            j=src.find('\n',i); i=j if j>=0 else n; continue
        if src.startswith('"""',i): in_str='"""'; out.append('"""'); i+=3; continue
        if c=='"': in_str='"'; out.append(c); i+=1; continue
        if c=="'":
            # char literal
            m=re.match(r"'(\\.|[^'\\])'",src[i:])
            if m: out.append(m.group(0)); i+=len(m.group(0)); continue
        out.append(c); i+=1
    return ''.join(out)

MODS=r'(?:(?:public|protected|internal|private|open|abstract|final|override|inline|noinline|crossinline|infix|operator|tailrec|external|suspend|actual|expect|const|lateinit|sealed|data|value|fun|enum|annotation|companion)\s+)*'
def bal(s,i,open_,close):
    depth=0
    j=i
    while j<len(s):
        c=s[j]
        if c==open_: depth+=1
        elif c==close:
            depth-=1
            if depth==0: return j
        j+=1
    return -1

entries=[]   # extension/top-level: (receiver,name,kind,tparams,params,ret,flags)
members=[]   # (owner,name,kind,tparams,params,ret,flags)
types={}     # name -> (tparams, supers)

def compact(s): return re.sub(r'\s+',' ',s).strip()

def parse_decl(s,pos,depth_kind):
    """s[pos:] starts at 'fun' or 'val'/'var'. returns dict or None, end position"""
    m=re.match(r'(fun|val|var)\b\s*',s[pos:])
    if not m: return None,pos+3
    kw=m.group(1); i=pos+m.end()
    tparams=''
    if s[i]=='<':
        j=bal(s,i,'<','>'); 
        if j<0: return None,i
        tparams=s[i:j+1]; i=j+1
        while s[i].isspace(): i+=1
    # receiver + name: scan until '(' (fun) or ':' '=' '\n' (val) at angle depth 0
    j=i;ang=0;par=0
    head_end=None
    while j<len(s):
        c=s[j]
        if c=='<': ang+=1
        elif c=='>' and s[j-1]!='-': ang=max(0,ang-1)
        elif c=='(':
            if kw=='fun' and ang==0 and par==0: head_end=j;break
            par+=1
        elif c==')': par-=1
        elif kw!='fun' and ang==0 and par==0 and c in ':={\n': head_end=j;break
        j+=1
    if head_end is None: return None,i
    head=compact(s[i:head_end])
    # split receiver/name at last '.' at angle+paren depth 0
    ang=0;par=0;dot=-1
    for k,c in enumerate(head):
        if c=='<': ang+=1
        elif c=='>' and head[k-1]!='-': ang=max(0,ang-1)
        elif c=='(': par+=1
        elif c==')': par-=1
        elif c=='.' and ang==0 and par==0: dot=k
    if dot>=0: recv=head[:dot].strip(); name=head[dot+1:].strip()
    else: recv=''; name=head.strip()
    name=name.strip('`')
    if not re.match(r'^[A-Za-z_][A-Za-z0-9_]*$',name): return None,head_end
    params='';ret='';k=head_end
    if kw=='fun':
        e=bal(s,head_end,'(',')')
        if e<0: return None,head_end
        params=compact(s[head_end+1:e]); k=e+1
        m2=re.match(r'\s*:\s*',s[k:])
        if m2:
            k+=m2.end(); j=k;ang=0;par=0
            while j<len(s):
                c=s[j]
                if c=='<': ang+=1
                elif c=='>' and s[j-1]!='-': ang=max(0,ang-1)
                elif c=='(' : par+=1
                elif c==')': par-=1
                elif ang==0 and par==0 and c in '{=\n': break
                j+=1
            ret=compact(s[k:j]); k=j
            if re.search(r'\bwhere\b',ret): tparams_where=True
            ret=re.sub(r'\s+where\b.*$','',ret)
    else:
        m2=re.match(r'\s*:\s*',s[head_end:])
        if m2:
            k=head_end+m2.end(); j=k;ang=0;par=0
            while j<len(s):
                c=s[j]
                if c=='<': ang+=1
                elif c=='>' and s[j-1]!='-': ang=max(0,ang-1)
                elif c=='(' : par+=1
                elif c==')': par-=1
                elif ang==0 and par==0 and c in '{=\n': break
                j+=1
            ret=compact(s[k:j]); k=j
    return dict(kw=kw,tparams=tparams,recv=recv,name=name,params=params,ret=ret),k

for f in files:
    raw=open(f,errors='ignore').read()
    src=strip_comments(raw)
    n=len(src);i=0;depth=0
    stack=[]  # (kind,name,depth_at_open)
    line_start=True
    # tokenise by scanning; detect declarations at depth 0 (top-level) or depth 1 within class/interface/object
    # Simple approach: iterate lines, track braces
    pos=0
    lines=src.split('\n')
    # rebuild offsets
    offs=[];o=0
    for l in lines: offs.append(o);o+=len(l)+1
    depth=0
    cur_type=None; type_depth=None
    li=0
    while li<len(lines):
        line=lines[li]
        stripped=line.strip()
        base=offs[li]
        # annotations lines preceding
        m=re.match(r'^\s*'+MODS+r'(fun|val|var)\b',line) if depth==0 or (cur_type and depth==type_depth+1) else None
        m_type=re.match(r'^\s*(?:@\w+(?:\([^)]*\))?\s+)*'+MODS+r'(class|interface|object)\s+(\w+)\s*(<[^{(]*?>)?\s*(?:\([^)]*\))?\s*(?::\s*([^{]*))?',line) if depth==0 else None
        prev_ann=' '.join(lines[max(0,li-3):li])
        deprecated='@Deprecated' in prev_ann or '@kotlin.Deprecated' in prev_ann or '@DeprecatedSinceKotlin' in prev_ann
        hidden = 'private ' in line.split('fun')[0] or 'internal ' in line.split('fun')[0] or 'private ' in line.split('val')[0].split('fun')[0]
        if m_type and not re.match(r'^\s*(?:private|internal)\b',line):
            tn=m_type.group(2); tp=m_type.group(3) or ''
            sup=m_type.group(4) or ''
            # supertypes may continue on following lines until '{'
            k=li
            while '{' not in lines[k] and k+1<len(lines) and sup.strip().endswith(',') or (m_type.group(4) is None and '{' not in lines[li] and False):
                k+=1; sup+=' '+lines[k]
            sup=sup.split('{')[0]
            supers=[]
            d=0;curr=''
            for ch in sup:
                if ch in '<(': d+=1
                elif ch in '>)': d-=1
                if ch==',' and d==0: supers.append(curr.strip());curr=''
                else: curr+=ch
            if curr.strip(): supers.append(curr.strip())
            supers=[re.sub(r'\(.*','',s_).strip() for s_ in supers]
            supers=[re.sub(r'^\s*(?:in|out)\s+','',s_) for s_ in supers if s_]
            types.setdefault(tn,(tp,supers))
            if '{' in line and '}' not in line.split('{',1)[1]:
                cur_type=tn; type_depth=depth
        if m and not hidden and not deprecated:
            # gather statement text starting at this line's decl keyword
            kwpos=base+m.end()-len(m.group(1))
            d,end=parse_decl(src,kwpos,None)
            if d and not (d['kw']!='fun' and False):
                flags=[]
                head_text=line[:m.end()]
                for fl in ('inline','infix','operator','suspend'):
                    if re.search(r'\b'+fl+r'\b',head_text): flags.append(fl)
                if depth==0:
                    if d['recv'] or True:
                        entries.append((d['recv'],d['name'],d['kw'],d['tparams'],d['params'],d['ret'],','.join(flags)))
                elif cur_type and depth==type_depth+1:
                    if 'override' not in head_text or True:
                        members.append((cur_type,d['name'],d['kw'],d['tparams'],d['params'],d['ret'],','.join(flags)))
        # update depth
        depth+= line.count('{')-line.count('}')
        if cur_type and depth<=type_depth: cur_type=None
        li+=1

# de-dup
def uniq(xs):
    seen=set();out=[]
    for x in xs:
        if x in seen: continue
        seen.add(x);out.append(x)
    return out
entries=uniq(entries);members=uniq(members)
top=[e for e in entries if e[0]]  # with receiver
print(len(entries),len(top),len(members),len(types),file=sys.stderr)
out=open(sys.argv[1],'w')
for t,(tp,sup) in sorted(types.items()):
    out.write('T\t%s\t%s\t%s\n'%(t,tp,','.join(sup)))
for m in sorted(members):
    out.write('M\t'+'\t'.join(m)+'\n')
for e in sorted(top):
    # `fun <T, R> R.contains(..) where R : Iterable<T>`: a bare type-parameter
    # receiver with a `where` bound cannot be matched without resolving bounds.
    if e[0]=='R': continue
    if re.match(r'^U(Byte|Short|Int|Long)Array',e[0]): continue
    out.write('E\t'+'\t'.join(e)+'\n')
out.close()

# Every declared public name (functions, properties, classes, typealiases):
# stdlib_names.txt, used to decide whether a bare name may come from the
# default imports.
names=set()
name_pat=re.compile(r'\b(?:fun|val|var|class|interface|object|typealias)\s+(?:<[^>]*>\s*)?(?:[\w.<>?, *]*\.)?`?([A-Za-z_][A-Za-z_0-9]*)`?')
for f in glob.glob(f'{root}/**/*.kt',recursive=True):
    for m in name_pat.finditer(open(f,errors='ignore').read()): names.add(m.group(1))
open(os.path.join(os.path.dirname(os.path.abspath(sys.argv[1])),'stdlib_names.txt'),'w').write("\n".join(sorted(names))+"\n")
