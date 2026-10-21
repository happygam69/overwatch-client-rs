"""Replay-emu: loader flat image + captured RUNTIME .data/.rdata overlaid, then run TLS cb[0].
Compares against the disk-only baseline (123.9M insns, zero writes, end rip 0x181a7e905).
Any MAIN-WRITE/DATA-WRITE or path divergence is the decrypt signal."""
import struct, sys
from unicorn import (Uc, UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE, UC_HOOK_MEM_WRITE,
                     UC_HOOK_MEM_READ, UC_HOOK_MEM_UNMAPPED, UcError)
import unicorn.x86_const as x
LOADER_BASE=0x180000000; MAIN_BASE=0x140000000; MAIN_SIZE=0x4000000
STACK_BASE=0x7FF000000000; STACK_SIZE=0x100000
SLOT_BASE=0x2A1A00000000; SENTINEL=0x7F0000F0000
TEB_BASE=0x7F000010000; PEB_BASE=0x7F000020000; LDR_BASE=0x7F000030000
RUN=r"D:/ow-dumps/live_cookie/run_20261021_023812"
log=[]
def note(s): log.append(s)
def main():
    steps=int(sys.argv[sys.argv.index("--steps")+1]) if "--steps" in sys.argv else 30_000_000
    u=Uc(UC_ARCH_X86, UC_MODE_64)
    flat=open(r"D:\ow-dumps\loader_disk_rva.bin","rb").read()
    u.mem_map(LOADER_BASE,(len(flat)+0xFFF)&~0xFFF); u.mem_write(LOADER_BASE,flat)
    note(f"mapped loader {len(flat):#x}")
    for name,rva,fn in (("data",0x1DF0000,RUN+"/data.bin"),("rdata",0x1D70000,RUN+"/rdata.bin")):
        d=open(fn,"rb").read()
        u.mem_write(LOADER_BASE+rva,d); note(f"overlay {name} {len(d):#x} at rva {rva:#x}")
    exe=open(r"E:\Overwatch\_retail_\Overwatch.exe","rb").read()
    pe=struct.unpack_from("<I",exe,0x3C)[0]; nsec=struct.unpack_from("<H",exe,pe+6)[0]
    opt=pe+0x18; sec_off=opt+struct.unpack_from("<H",exe,pe+20)[0]
    u.mem_map(MAIN_BASE,MAIN_SIZE)
    for i in range(nsec):
        s=sec_off+i*40; vsz,va,rsz,ro=struct.unpack_from("<IIII",exe,s+8)
        if rsz and va+min(rsz,vsz)<=MAIN_SIZE:
            u.mem_write(MAIN_BASE+va,exe[ro:ro+min(rsz,vsz)])
    note("main ciphertext mapped")
    u.mem_map(STACK_BASE,STACK_SIZE); u.mem_map(SLOT_BASE,0x100000)
    u.mem_write(SLOT_BASE,b"\xC3"*0x1000); u.mem_map(SENTINEL,0x1000); u.mem_write(SENTINEL,b"\xC3"*0x1000)
    u.mem_map(TEB_BASE,0x1000); u.mem_map(PEB_BASE,0x1000); u.mem_map(LDR_BASE,0x10000)
    u.mem_write(TEB_BASE+0x60,struct.pack("<Q",PEB_BASE))
    u.mem_write(PEB_BASE+0x10,struct.pack("<Q",LOADER_BASE)); u.mem_write(PEB_BASE+0x18,struct.pack("<Q",LDR_BASE))
    u.reg_write(x.UC_X86_REG_FS_BASE,TEB_BASE); u.reg_write(x.UC_X86_REG_GS_BASE,TEB_BASE)
    st={"n":0}; writes=[]; main_exec=0; reads={}
    def hc(uu,addr,size,ud):
        st["n"]+=1
        if st["n"]%20_000_000==0: note(f"@{st['n']} rip {addr:#x}")
        if MAIN_BASE<=addr<MAIN_BASE+MAIN_SIZE:
            main_exec+=1
            if main_exec<=10: note(f"MAIN-EXEC at {addr:#x}")
    def hw(uu,access,address,size,value,ud):
        if LOADER_BASE+0x1DC00000<=address<LOADER_BASE+0x1E50000:
            writes.append((address-LOADER_BASE,size,value)); note(f"DATA-WRITE {(address-LOADER_BASE):#x} sz{size} val {value:#x}")
        elif MAIN_BASE<=address<MAIN_BASE+MAIN_SIZE:
            writes.append(("main",address-MAIN_BASE,size,value)); note(f"MAIN-WRITE {(address-MAIN_BASE):#x} sz{size} val {value:#x}")
    def hr(uu,access,address,size,value,ud):
        pg=(address-LOADER_BASE)>>12 if LOADER_BASE<=address<LOADER_BASE+0x2410000 else address
        reads[pg]=reads.get(pg,0)+1
    def hu(uu,access,address,size,value,ud):
        note(f"UNMAPPED at {address:#x} rip {uu.reg_read(x.UC_X86_REG_RIP):#x}"); uu.mem_map(address&~0xFFF,0x1000); return True
    u.hook_add(UC_HOOK_CODE,hc); u.hook_add(UC_HOOK_MEM_WRITE,hw,begin=LOADER_BASE,end=LOADER_BASE+0x2410000)
    u.hook_add(UC_HOOK_MEM_READ,hr,begin=LOADER_BASE,end=LOADER_BASE+0x2410000)
    u.hook_add(UC_HOOK_MEM_UNMAPPED,hu)
    rsp=STACK_BASE+0x80000; u.mem_write(rsp,struct.pack("<Q",SENTINEL))
    u.reg_write(x.UC_X86_REG_RSP,rsp); u.reg_write(x.UC_X86_REG_RCX,LOADER_BASE)
    u.reg_write(x.UC_X86_REG_RDX,1); u.reg_write(x.UC_X86_REG_R8,0)
    try:
        u.emu_start(LOADER_BASE+0x48FFD0,SENTINEL,timeout=120_000_000,count=steps)
        note(f"emu returned after {st['n']} insns; last rip {u.reg_read(x.UC_X86_REG_RIP):#x}")
    except UcError as e:
        note(f"UC ERROR at {u.reg_read(x.UC_X86_REG_RIP):#x}: {e}")
    note(f"writes={len(writes)} main_exec_hits={main_exec}")
    open(r"D:\ow-dumps\emu_tls_replay_log.txt","w").write("\n".join(log))
    print("\n".join(log[-25:])); print(f"... {len(log)} lines")
    print("== top read pages ==")
    for k,v in sorted(reads.items(),key=lambda kv:-kv[1])[:15]: print(f"  {k:#x}: {v}")
if __name__=="__main__": main()
