# sonde: names the place in the program that sent each S3 request.
#
# Put on PYTHONPATH by `sonde run --callers` (and the app's "Name the code"),
# only then. For boto3 / botocore it adds one header to each S3 request,
# before it is signed: x-sonde-caller: <file>:<line> <function>, the first
# frame of the program's own code (not the standard library's, not an
# installed package's). sonde reads it and never forwards it to the store.
# Nothing else changes; a sitecustomize of the program's own still runs.
import os
import sys


def _sonde_place(lib, here):
    f = sys._getframe(2)
    while f is not None:
        p = f.f_code.co_filename.replace("\\", "/")
        if not p.startswith("<") and not p.startswith(lib) and not p.startswith(here):
            try:
                shown = os.path.relpath(p)
            except ValueError:
                shown = p
            if shown.startswith(".."):
                shown = p
            return "%s:%d %s" % (shown, f.f_lineno, f.f_code.co_name)
        f = f.f_back
    return None


def _sonde_install():
    import importlib.util
    import sysconfig

    if importlib.util.find_spec("botocore") is None:
        return
    import botocore.session

    here = os.path.dirname(os.path.abspath(__file__)).replace("\\", "/")
    paths = sysconfig.get_paths()
    lib = tuple(
        sorted(
            {paths[k].replace("\\", "/") for k in ("stdlib", "platstdlib", "purelib", "platlib") if paths.get(k)},
            key=len,
            reverse=True,
        )
    )

    def add(request, **kwargs):
        place = _sonde_place(lib, here)
        if place:
            request.headers["x-sonde-caller"] = place[:200]

    init = botocore.session.Session.__init__

    def __init__(self, *args, **kwargs):
        init(self, *args, **kwargs)
        self.register("before-sign.s3", add, unique_id="sonde-caller")

    botocore.session.Session.__init__ = __init__


def _sonde_chain():
    # the program's own sitecustomize, which this one shadows
    import importlib.machinery
    import importlib.util

    here = os.path.dirname(os.path.abspath(__file__))
    rest = [p for p in sys.path if os.path.abspath(p or ".") != here]
    spec = importlib.machinery.PathFinder.find_spec("sitecustomize", rest)
    if spec and spec.origin and os.path.dirname(os.path.abspath(spec.origin)) != here:
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)


try:
    _sonde_install()
except Exception:
    pass
_sonde_chain()
