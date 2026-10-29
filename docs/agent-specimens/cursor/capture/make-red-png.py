# Writes a solid red 64x64 PNG (stdlib only): python3 -B make-red-png.py OUT.png
import sys,zlib,struct
def ch(t,d): c=struct.pack(">I",len(d))+t+d; return c+struct.pack(">I",zlib.crc32(t+d)&0xffffffff)
raw=b"".join(b"\x00"+b"\xff\x00\x00"*64 for _ in range(64))
open(sys.argv[1],"wb").write(b"\x89PNG\r\n\x1a\n"+ch(b"IHDR",struct.pack(">IIBBBBB",64,64,8,2,0,0,0))+ch(b"IDAT",zlib.compress(raw))+ch(b"IEND",b""))
