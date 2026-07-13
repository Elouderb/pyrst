# bare_import_demo — Python-style `import <package>` (card 89408863)

Proves the bare package-import ergonomic end to end: `import numpyrs` /
`import kodiak` resolving through the package's `__init__.pyrs` entry file, with
each qualified `<pkg>.<Name>` lowering to its **true** defining submodule.

- `numpyrs_bare.pyrs` — self-contained (numpyrs has no external deps). Uses
  `numpyrs.array(...)` (→ `numpyrs.constructors`), `numpyrs.sqrt(...)` (→
  `numpyrs.ufunc`), and qualified class construction `numpyrs.NDArray(...)` (→
  `numpyrs.ndarray`).
- `kodiak_bare.pyrs` — the headline `import kodiak; kodiak.read_csv("sales.csv")`
  (→ `kodiak.io`) plus a re-exported free function `kodiak.frame_equals(...)` (→
  `kodiak.frame`). Pulls in kodiak's full closure (numpyrs, dateutil, tzdata).
- `sales.csv` — sample data for the kodiak demo.

## Run against the env store

```sh
pyrst venv && source .pyrstenv/activate
pyrst install <repo>/extern/packages/numpyrs      # for numpyrs_bare
pyrst install <repo>/extern/packages/kodiak       # for kodiak_bare (+ its deps)
pyrst build numpyrs_bare.pyrs && ./numpyrs_bare
pyrst build kodiak_bare.pyrs  && ./kodiak_bare
```

## Run reproducibly in-repo (no env)

```sh
PYRST_PATH=<repo>/extern/packages pyrst build numpyrs_bare.pyrs && ./numpyrs_bare
PYRST_PATH=<repo>/extern/packages pyrst build kodiak_bare.pyrs  && ./kodiak_bare
```

The pre-existing dotted form (`from kodiak.series import Series`) still works
unchanged and never consults `__init__.pyrs`.
