"""Send one command to a running imgr (stdlib only).

  python imgr_ctl.py state
  python imgr_ctl.py query q="#pixiv score>=4"
  python imgr_ctl.py select index=[0,1,2]
  python imgr_ctl.py score n=5
  python imgr_ctl.py tag add=["pixiv"]
  python imgr_ctl.py screenshot path=C:/tmp/shot.png
  python imgr_ctl.py layout
  python imgr_ctl.py '{"cmd":"view","mode":"compare"}'

Values are parsed as JSON when possible, else used as text.
Commands: state query list select score tag trash undo view key press type
mouse wheel thumb window layout screenshot rescan import save_filter notice
lib lib_show rename export panel modal focus save quit
albums album_new album_rename album_delete album_put album_open drop
  albums     = album list, counts, the album shown     album_open id=3 | id=none | (top)
  album_put  = put the selected pictures into album id=  (no id = out of every album)
  drop       = like dropping files on the window          drop files=["C:/x/a.png"]
  key   = run an imgr shortcut directly          key key=F2
  press = real key event through the UI           press key=Enter ctrl=true
  mouse = op move|click|dblclick|right|down|up|drag x= y= (to=[x,y] steps=8)
  layout = widget rectangles of the last frame (use them for mouse x/y)
Set IMGR_PORT to talk to a test instance on another port.
"""
import json
import os
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))


def port():
    if os.environ.get("IMGR_PORT"):
        return int(os.environ["IMGR_PORT"])
    # data\ctl.port of the imgr root: IMGR_ROOT, this folder, or the one above (tools\)
    roots = [os.environ.get("IMGR_ROOT", ""), HERE, os.path.dirname(HERE)]
    for r in roots:
        try:
            with open(os.path.join(r, "data", "ctl.port")) as f:
                return int(f.read().strip())
        except Exception:
            pass
    return 47811


def send(body, timeout=25):
    req = urllib.request.Request(f"http://127.0.0.1:{port()}/cmd", data=json.dumps(body).encode("utf-8"), method="POST")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))


def main(argv):
    if not argv:
        print(__doc__)
        return 1
    if argv[0].startswith("{"):
        body = json.loads(argv[0])
    else:
        body = {"cmd": argv[0]}
        for a in argv[1:]:
            k, _, v = a.partition("=")
            try:
                body[k] = json.loads(v)
            except ValueError:
                body[k] = v
    print(json.dumps(send(body), ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.exit(main(sys.argv[1:]))
