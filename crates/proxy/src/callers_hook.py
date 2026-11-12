# sonde: names the place in the program that sent each S3 request.
#
# Put on PYTHONPATH by `sonde run` (unless --no-callers) and the app's Run.
# For boto3 / botocore it adds one header to each S3 request to sonde,
# before it is signed: x-sonde-caller: <file>:<line> <function>, the first
# frame of the program's own code (not the standard library's, not an
# installed package's). A request sent from a worker thread (boto3's
# transfer manager) is named by the place that handed the work to the pool.
# sonde reads the header and never forwards it to the store.
#
# Cheap: nothing is imported at start; the hook is installed when the
# program itself imports botocore. Safe: any failure leaves the program as
# it was, and a sitecustomize of the program's own still runs.
import os
import sys


def _sonde_hook():
    import threading

    here = os.path.dirname(os.path.abspath(__file__)).replace("\\", "/")
    local = threading.local()
    lib = []

    def libs():
        if lib:
            return lib[0]
        import site
        import sysconfig

        paths = sysconfig.get_paths()
        dirs = {paths[k] for k in ("stdlib", "platstdlib", "purelib", "platlib") if paths.get(k)}
        # installed packages live in more places than sysconfig names (Debian's
        # dist-packages, the user's site)
        try:
            dirs.update(site.getsitepackages())
        except Exception:
            pass
        try:
            dirs.add(site.getusersitepackages())
        except Exception:
            pass
        dirs.update(p for p in sys.path if p and ("site-packages" in p or "dist-packages" in p))
        lib.append(tuple(sorted({d.replace("\\", "/") for d in dirs if d}, key=len, reverse=True)))
        return lib[0]

    def place():
        skip = libs()
        f = sys._getframe(1)
        while f is not None:
            p = f.f_code.co_filename.replace("\\", "/")
            if not p.startswith("<") and not p.startswith(skip) and not p.startswith(here):
                try:
                    shown = os.path.relpath(p)
                except ValueError:
                    shown = p
                if shown.startswith(".."):
                    shown = p
                # one separator on every system (Windows' relpath gives backslashes)
                shown = shown.replace("\\", "/")
                at = "%s:%d %s" % (shown, f.f_lineno, f.f_code.co_name)
                # a header is ASCII: other characters are written as escapes
                return at.encode("ascii", "backslashreplace").decode("ascii")
            f = f.f_back
        # a worker thread: the place that handed it the work
        return getattr(local, "place", None)

    # only requests to sonde carry the header: never one to another store
    def to_sonde(url):
        from urllib.parse import urlsplit

        try:
            endpoint = urlsplit(os.environ.get("AWS_ENDPOINT_URL", ""))
            u = urlsplit(str(url))
            host = u.hostname or ""
            return bool(endpoint.hostname) and u.port == endpoint.port and (host == endpoint.hostname or host.endswith("." + endpoint.hostname))
        except ValueError:
            return False

    def add(request, signature_version=None, **kwargs):
        # a presigned URL is used by someone else, without this header; an
        # unsigned request (botocore.UNSIGNED, not a string) has nothing to sign
        if not isinstance(signature_version, str) or "query" in signature_version or "presign" in signature_version:
            return
        if not to_sonde(getattr(request, "url", "")):
            return
        try:
            at = place()
        except Exception:
            return
        if at:
            request.headers["x-sonde-caller"] = at[:200]

    def patch_session(module):
        init = module.Session.__init__

        def __init__(self, *args, **kwargs):
            init(self, *args, **kwargs)
            self.register("before-sign.s3", add, unique_id="sonde-caller")

        module.Session.__init__ = __init__
        # work handed to a pool runs under the place that handed it
        from concurrent.futures import ThreadPoolExecutor

        submit = ThreadPoolExecutor.submit

        def sonde_submit(self, fn, *args, **kwargs):
            try:
                at = place()
            except Exception:
                at = None
            if at is None:
                return submit(self, fn, *args, **kwargs)

            def under(*a, **k):
                before = getattr(local, "place", None)
                local.place = at
                try:
                    return fn(*a, **k)
                finally:
                    local.place = before

            return submit(self, under, *args, **kwargs)

        ThreadPoolExecutor.submit = sonde_submit

    class Loader:
        def __init__(self, inner):
            self.inner = inner

        def __getattr__(self, name):
            return getattr(self.inner, name)

        def create_module(self, spec):
            return self.inner.create_module(spec)

        def exec_module(self, module):
            self.inner.exec_module(module)
            try:
                patch_session(module)
            except Exception:
                pass

    class Finder:
        # waits for botocore.session to be imported, then patches it once
        def find_spec(self, name, path=None, target=None):
            if name != "botocore.session":
                return None
            try:
                sys.meta_path.remove(self)
            except ValueError:
                pass
            import importlib.util

            try:
                spec = importlib.util.find_spec(name)
            except Exception:
                return None
            if spec is None or not hasattr(spec.loader, "exec_module"):
                return spec
            spec.loader = Loader(spec.loader)
            return spec

    sys.meta_path.insert(0, Finder())


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
    _sonde_hook()
except Exception:
    pass
_sonde_chain()
