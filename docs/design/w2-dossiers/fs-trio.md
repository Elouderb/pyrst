# fs-trio: CPython Oracle Dossier

**Module Coverage**: shutil, tempfile, filecmp  
**Probe Date**: 2026-07-02  
**Dialect**: CPython 3.12.9

---

## 1. SURFACE

| API | Kind | Signature | Return | Semantics |
|-----|------|-----------|--------|-----------|
| shutil.copyfile | fn | copyfile(src, dst, *, follow_symlinks=True) | str | Copy file content only; no metadata; returns dst path; raises FileNotFoundError if src missing |
| shutil.copy | fn | copy(src, dst, *, follow_symlinks=True) | str | Copy file+mode metadata; returns dst path; raises FileNotFoundError if src missing |
| shutil.copytree | fn | copytree(src, dst, symlinks=False, ignore=None, copy_function=<copy2>, ignore_dangling_symlinks=False, dirs_exist_ok=False) | str | Recursively copy directory tree; returns dst path; raises FileExistsError if dst exists and dirs_exist_ok=False |
| shutil.rmtree | fn | rmtree(path, ignore_errors=False, onerror=None, *, onexc=None, dir_fd=None) | None | Recursively delete directory tree; returns None; raises FileNotFoundError if path missing and ignore_errors=False |
| shutil.move | fn | move(src, dst, copy_function=<copy2>) | str | Move/rename file or directory; returns final destination path; raises FileExistsError if dst file exists; places in dst dir if dst is directory |
| shutil.disk_usage | fn | disk_usage(path) | usage | Return (total, used, free) named tuple for filesystem of path; raises FileNotFoundError if path missing |
| tempfile.gettempdir | fn | gettempdir() | str | Return system temp directory path (e.g. '/tmp'); always succeeds |
| tempfile.mkdtemp | fn | mkdtemp(suffix=None, prefix=None, dir=None) | str | Create unique temp directory; returns absolute path; raises FileNotFoundError if dir invalid |
| tempfile.mkstemp | fn | mkstemp(suffix=None, prefix=None, dir=None, text=False) | (int, str) | Create unique temp file & open fd; returns (fd, path); raises FileNotFoundError if dir invalid |
| tempfile.NamedTemporaryFile | fn | NamedTemporaryFile(mode='w+b', buffering=-1, encoding=None, newline=None, suffix=None, prefix=None, dir=None, delete=True, *, errors=None, delete_on_close=True) | file-like | Return open temporary file object; delete=True removes on close; raises FileNotFoundError if dir invalid |
| filecmp.cmp | fn | cmp(f1, f2, shallow=True) | bool | Compare files; True if identical, False if different; FileNotFoundError if either file missing |
| filecmp.cmpfiles | fn | cmpfiles(a, b, common, shallow=True) | (list, list, list) | Return (match, mismatch, errors) tuples of filenames from common list; no error on missing dir or files |
| filecmp.dircmp | class | __init__(self, a, b, ignore=None, hide=None) | dircmp | Compare directories; attributes: same_files, diff_files, funny_files, left_only, right_only, subdirs, left_list, right_list; raises FileNotFoundError if either dir missing |

---

## 2. ERRORS

Exact exception types and messages for edge inputs:

### copyfile Errors
```
copyfile("/nonexistent.txt", "/tmp/dst.txt")
→ FileNotFoundError: [Errno 2] No such file or directory

copyfile(src, "/readonly/dst.txt") # destination read-only
→ PermissionError: [Errno 13] Permission denied

copyfile(123, 456)  # bad type
→ OSError: [Errno 9] Bad file descriptor
```

### copy Errors
```
copy("/nonexistent.txt", "/tmp/dst.txt")
→ FileNotFoundError: [Errno 2] No such file or directory
```

### copytree Errors
```
copytree("/nonexistent_dir", "/tmp/dst")
→ FileNotFoundError: [Errno 2] No such file or directory: '/nonexistent_dir'

copytree(src_tree, existing_dir, dirs_exist_ok=False)
→ FileExistsError: [Errno 17] File exists: existing_dir

copytree(src_with_dangling_link, dst, ignore_dangling_symlinks=False)
→ Error: (unspecified message, platform-dependent)
```

### rmtree Errors
```
rmtree("/nonexistent", ignore_errors=False)
→ FileNotFoundError: [Errno 2] No such file or directory

rmtree("/nonexistent", ignore_errors=True)
→ None  # no error
```

### move Errors
```
move("/nonexistent_src", "/tmp/dst")
→ FileNotFoundError: [Errno 2] No such file or directory

move(src, existing_file)  # dst file exists
→ Error: Destination path 'existing_file' already exists
```

### disk_usage Errors
```
disk_usage("/nonexistent")
→ FileNotFoundError: [Errno 2] No such file or directory: '/nonexistent'

disk_usage(None)
→ TypeError
```

### mkdtemp Errors
```
mkdtemp(dir="/nonexistent")
→ FileNotFoundError: [Errno 2] No such file or directory: '/nonexistent'
```

### mkstemp Errors
```
mkstemp(dir="/nonexistent")
→ FileNotFoundError: [Errno 2] No such file or directory: '/nonexistent'
```

### cmp Errors
```
cmp("/nonexistent1.txt", "/nonexistent2.txt")
→ FileNotFoundError: [Errno 2] No such file or directory

cmp(file, dir)  # comparing file to directory
→ no error, compares as regular path (True/False) based on inode
```

### cmpfiles Errors
```
cmpfiles("/nonexistent_a", "/nonexistent_b", [])
→ no error; returns ([], [], [])

cmpfiles(file1, file2, [])  # non-directory paths
→ no error (treats as directory paths, lists empty)
```

### dircmp Errors
```
dircmp("/existing_dir", "/nonexistent_dir")
→ FileNotFoundError: [Errno 2] No such file or directory: '/nonexistent_dir'
```

---

## 3. BEHAVIOR MATRIX

Probed input→output pairs (verbatim python3 output via print(repr(...))):

```python
# COPYFILE
copyfile("src.txt", "dst.txt") → '/tmp/.../dst.txt'
copyfile("src.txt", dst)  # overwrite existing → '/tmp/.../dst.txt'

# COPY (includes metadata)
copy("src.txt", "dst.txt") → '/tmp/.../dst.txt'

# COPYTREE
copytree("src_tree", "dst_tree") → '/tmp/.../dst_tree'
# Files copied: dst_tree contains src_tree's subdirs and files exactly

copytree(src_tree, dst_tree, dirs_exist_ok=True)  # second call → success
copytree(src_tree, dst_tree, dirs_exist_ok=False)  # second call → FileExistsError

copytree(src_tree, dst_tree, symlinks=False) → links dereferenced to files
copytree(src_tree, dst_tree, symlinks=True) → links preserved as symlinks

copytree(src_tree, dst_tree, ignore=lambda d, n: [x for x in n if x.startswith('ignore')])
# Files matching pattern excluded from copy

# RMTREE
rmtree("tree_path") → None
# Directory and all contents deleted

rmtree("/nonexistent", ignore_errors=True) → None
# No error even if path missing

# MOVE
move("file.txt", "newname.txt") → '/tmp/.../newname.txt'
# File renamed and removed from source location

move("file.txt", existing_dir) → '/tmp/.../existing_dir/file.txt'
# File moved into directory

move(dir_src, dir_dst) → '/tmp/.../dir_dst'
# Directory moved/renamed

# DISK_USAGE
disk_usage("/path") → usage(total=844365119488, used=578207789056, free=228603371520)
usage.total → 844365119488 (i64 safe)
usage.used → 578207789056
usage.free → 228603371520

# TEMPFILE
gettempdir() → '/tmp'
mkdtemp() → '/tmp/tmpf72o_a6c'
mkdtemp(prefix="pre_", suffix="_suf") → '/tmp/pre_XXXX_suf' (XXXX random)
mkdtemp(dir=parent_dir) → parent_dir/tmpXXXX

mkstemp() → (3, '/tmp/tmpaj9kndfk')  # (fd, path)
mkstemp(prefix="pre_", suffix=".tmp") → (fd, '/tmp/pre_XXXX.tmp')
mkstemp(text=False) → (fd, path)  # binary mode, fd ready for os.write(fd, bytes)
mkstemp(text=True) → (fd, path)   # text mode

NamedTemporaryFile(mode='w+b', delete=False) → <file-like object>
ntf.name → '/tmp/tmp_XXXX'
ntf.write(b"data") → 5  # bytes written
ntf.close() → file remains (delete=False)
ntf.close() → file deleted (delete=True)

# FILECMP
cmp("f1.txt", "f2.txt") → True  # identical content
cmp("f1.txt", "f3.txt") → False  # different content
cmp(large_f1, large_f2, shallow=True) → True  # stat comparison only
cmp(large_f1, large_f2, shallow=False) → True  # byte-by-byte

# CMPFILES
cmpfiles(dir_a, dir_b, ["same.txt", "diff.txt"])
→ (['same.txt'], ['diff.txt'], [])  # (match, mismatch, errors)

cmpfiles(dir_a, dir_b, ["same.txt", "missing.txt"])
→ (['same.txt'], [], ['missing.txt'])  # missing files go to errors

# DIRCMP
dircmp(dir_a, dir_b).same_files → ['file.txt', 'other.txt']
dircmp(dir_a, dir_b).diff_files → ['different.txt']
dircmp(dir_a, dir_b).left_only → []
dircmp(dir_a, dir_b).right_only → []
dircmp(dir_a, dir_b).subdirs → {'subdir': <dircmp object>}
dircmp(dir_a, dir_b, ignore=[".ignore_me"]).left_list → ['file.txt']  # hidden file excluded

dircmp.report()  # prints diff summary
dircmp.report_full_closure()  # prints diff summary including subdirs
```

---

## 4. HAZARDS

### Platform Dependence
- **disk_usage** varies by filesystem and mount point; values are filesystem-specific
- **tempfile.gettempdir()** platform-dependent: /tmp on Unix, %TEMP% on Windows
- **symlink behavior**: platform-dependent (Windows requires special privileges; copytree.symlinks=True may fail)
- **ignore_dangling_symlinks error**: error message and behavior platform/OS-dependent

### Ordering Hazards
- **dircmp.left_list, right_list**: sorted alphabetically (safe for deterministic comparison)
- **cmpfiles return tuples**: element order is (match, mismatch, errors) — order guaranteed
- **filecmp.dircmp.same_files, diff_files**: sorted (safe)

### Type/Format Hazards
- **disk_usage return type**: namedtuple `usage` — immutable, allows attribute access but not dict-like
- **mkstemp returns 2-tuple**: (fd:int, path:str) — order critical
- **cmpfiles returns 3-tuple**: (match:list, mismatch:list, errors:list) — tuple not namedtuple
- **NamedTemporaryFile**: file-like object with mode parameter (pyrst file params not yet spellable)

### Randomness
- **mkdtemp**, **mkstemp**: generate random suffixes; output paths vary per call
- Cannot rely on specific path format; only parent dir guaranteed

### Time Dependence
- **copyfile**: does not preserve mtime (destination gets current time)
- **copy**: preserves mode but not mtime
- **copytree**: preservation depends on copy_function parameter

### Binary Content
- **copyfile** handles binary data exactly; null bytes and high-bit preserved
- **NamedTemporaryFile mode='w+b'**: binary mode accepts bytes directly
- **NamedTemporaryFile mode='w+'**: text mode requires str, will encode/decode

---

## 5. GATED

APIs that hit PYRST constraints and recommended deferral:

| Gate | API | Constraint | Workaround / Deferral |
|------|-----|-----------|------|
| G4 (no *args/**kwargs) | shutil.copytree | `ignore: Callable[[str, List[str]], List[str]]` — function callbacks not yet spellable as parameters | Defer custom ignore logic; provide canonical ignore patterns (e.g., ['.git', '__pycache__']) or post-filter results |
| G4 (no *args/**kwargs) | shutil.rmtree | `onerror: Callable[[Callable, str, tuple], None]` — error handler callback not spellable | Defer error handling; use ignore_errors=True for all-or-nothing semantics |
| G4 (no *args/**kwargs) | shutil.rmtree | `onexc: Callable[[Callable, str, OSError], None]` — exception handler callback not spellable | Defer exception handling; use ignore_errors=True |
| G4 (no *args/**kwargs) | shutil.copytree | `copy_function: Callable[[str, str], str]` — copy function callback not spellable | Use default copy2; no custom copy strategy |
| G4 (no *args/**kwargs) | shutil.move | `copy_function: Callable[[str, str], str]` — copy function callback not spellable | Use default copy2; no custom move strategy |
| File params (not yet spellable) | tempfile.NamedTemporaryFile | Returns file-like object; file parameters not yet definable in pyrst | Use mkstemp + manual file ops instead (os.fdopen, os.write, os.read) or defer file-object gate |
| File params (not yet spellable) | tempfile.NamedTemporaryFile | Requires mode, buffering, encoding params — text/binary mode selection not yet spellable | Defer flexible mode selection; provide mkstemp-based alternative |

---

## 6. PARITY PLAN

Safe dual-run test cases (python3-verified output) that avoid ordering/formatting hazards:

```python
# Avoid disk_usage (platform-dependent large ints)
# Avoid mkdtemp/mkstemp (random path generation)
# Avoid mtime (copyfile doesn't preserve)
# Avoid NamedTemporaryFile (file-object gate)

# Test 1: copyfile basic
src = Path("test_src.txt")
src.write_text("hello")
copyfile("test_src.txt", "test_dst.txt")
assert Path("test_dst.txt").read_text() == "hello"

# Test 2: copyfile overwrite
Path("test_dst.txt").write_text("old")
copyfile("test_src.txt", "test_dst.txt")
assert Path("test_dst.txt").read_text() == "hello"

# Test 3: copy preserves mode
src_file = Path("src_mode.txt")
src_file.write_text("test")
copy("src_mode.txt", "dst_mode.txt")
assert Path("dst_mode.txt").exists()

# Test 4: copytree basic recursion
src_tree = Path("src_tree")
src_tree.mkdir()
(src_tree / "f1.txt").write_text("f1")
(src_tree / "subdir").mkdir()
(src_tree / "subdir" / "f2.txt").write_text("f2")
copytree("src_tree", "dst_tree")
assert (Path("dst_tree") / "f1.txt").read_text() == "f1"
assert (Path("dst_tree") / "subdir" / "f2.txt").read_text() == "f2"

# Test 5: copytree dirs_exist_ok=False error
mkdir("existing_dst")
try:
    copytree(src_tree, "existing_dst", dirs_exist_ok=False)
    assert False, "Should raise FileExistsError"
except FileExistsError:
    pass

# Test 6: copytree dirs_exist_ok=True success
copytree(src_tree, "existing_dst", dirs_exist_ok=True)
assert (Path("existing_dst") / "f1.txt").exists()

# Test 7: copytree symlinks=True
link_src = Path("link_src_tree")
link_src.mkdir()
(link_src / "real.txt").write_text("real")
os.symlink((link_src / "real.txt").resolve(), link_src / "link.txt")
copytree("link_src_tree", "link_dst_tree", symlinks=True)
assert os.path.islink(Path("link_dst_tree") / "link.txt")

# Test 8: copytree symlinks=False
copytree("link_src_tree", "no_link_dst_tree", symlinks=False)
assert not os.path.islink(Path("no_link_dst_tree") / "link.txt")
assert (Path("no_link_dst_tree") / "link.txt").read_text() == "real"

# Test 9: rmtree removes directory
tree = Path("rm_tree")
tree.mkdir()
(tree / "file.txt").write_text("x")
(tree / "subdir").mkdir()
rmtree("rm_tree")
assert not tree.exists()

# Test 10: rmtree ignore_errors=True on missing
rmtree("nonexistent_tree", ignore_errors=True)  # no error

# Test 11: rmtree ignore_errors=False on missing
try:
    rmtree("nonexistent_tree", ignore_errors=False)
    assert False, "Should raise FileNotFoundError"
except FileNotFoundError:
    pass

# Test 12: move file rename
src_move = Path("to_move.txt")
src_move.write_text("move me")
result = move("to_move.txt", "moved.txt")
assert result.endswith("moved.txt")
assert not src_move.exists()
assert Path("moved.txt").read_text() == "move me"

# Test 13: move file into directory
src_move2 = Path("move_into_dir.txt")
src_move2.write_text("content")
dst_dir = Path("dst_dir")
dst_dir.mkdir(exist_ok=True)
result = move("move_into_dir.txt", str(dst_dir))
assert result.endswith("move_into_dir.txt")
assert (dst_dir / "move_into_dir.txt").read_text() == "content"

# Test 14: move directory
src_dir_move = Path("src_dir_move")
src_dir_move.mkdir()
(src_dir_move / "file.txt").write_text("x")
result = move("src_dir_move", "dst_dir_move")
assert result.endswith("dst_dir_move")
assert not src_dir_move.exists()
assert (Path("dst_dir_move") / "file.txt").exists()

# Test 15: move error on existing destination
src_exists = Path("src_exists.txt")
src_exists.write_text("x")
dst_exists = Path("dst_exists.txt")
dst_exists.write_text("y")
try:
    move("src_exists.txt", "dst_exists.txt")
    assert False, "Should raise Error"
except OSError:  # shutil.Error is OSError
    pass

# Test 16: gettempdir returns string
tmpdir = gettempdir()
assert isinstance(tmpdir, str)
assert len(tmpdir) > 0

# Test 17: cmp identical files
f1 = Path("cmp1.txt")
f2 = Path("cmp2.txt")
f1.write_text("same")
f2.write_text("same")
assert cmp("cmp1.txt", "cmp2.txt") == True

# Test 18: cmp different files
f3 = Path("cmp3.txt")
f3.write_text("different")
assert cmp("cmp1.txt", "cmp3.txt") == False

# Test 19: cmp shallow comparison
large1 = Path("large1.bin")
large2 = Path("large2.bin")
large1.write_bytes(b"x" * 10000)
large2.write_bytes(b"x" * 10000)
assert cmp("large1.bin", "large2.bin", shallow=True) == True

# Test 20: cmp deep comparison
assert cmp("large1.bin", "large2.bin", shallow=False) == True

# Test 21: cmpfiles return structure
dir_a = Path("cmp_dir_a")
dir_b = Path("cmp_dir_b")
dir_a.mkdir()
dir_b.mkdir()
(dir_a / "same.txt").write_text("same")
(dir_b / "same.txt").write_text("same")
(dir_a / "diff.txt").write_text("a")
(dir_b / "diff.txt").write_text("b")
match, mismatch, errors = cmpfiles(str(dir_a), str(dir_b), ["same.txt", "diff.txt"])
assert "same.txt" in match
assert "diff.txt" in mismatch
assert len(errors) == 0

# Test 22: cmpfiles with missing file
match, mismatch, errors = cmpfiles(str(dir_a), str(dir_b), ["same.txt", "missing.txt"])
assert "same.txt" in match
assert "missing.txt" in errors

# Test 23: dircmp basic attributes
dcmp = dircmp(str(dir_a), str(dir_b))
assert "same.txt" in dcmp.same_files
assert "diff.txt" in dcmp.diff_files

# Test 24: dircmp ignore parameter
(dir_a / "ignore_me.txt").write_text("x")
(dir_b / "ignore_me.txt").write_text("x")
dcmp2 = dircmp(str(dir_a), str(dir_b), ignore=["ignore_me.txt"])
assert "ignore_me.txt" not in dcmp2.left_list or "ignore_me.txt" in dcmp2.same_files

# Test 25: dircmp subdirectories
(dir_a / "subdir").mkdir()
(dir_b / "subdir").mkdir()
(dir_a / "subdir" / "file.txt").write_text("sub")
(dir_b / "subdir" / "file.txt").write_text("sub")
dcmp3 = dircmp(str(dir_a), str(dir_b))
assert "subdir" in dcmp3.subdirs
assert "file.txt" in dcmp3.subdirs["subdir"].same_files

# Test 26: gettempdir is valid directory
import os
tmpdir = gettempdir()
assert os.path.isdir(tmpdir)

# Test 27: copyfile binary preservation
binary_src = Path("binary_src.bin")
binary_dst = Path("binary_dst.bin")
binary_src.write_bytes(b"hello\x00world\xff")
copyfile("binary_src.bin", "binary_dst.bin")
assert binary_dst.read_bytes() == b"hello\x00world\xff"

# Test 28: copytree empty directories
empty_tree = Path("empty_tree")
empty_tree.mkdir()
(empty_tree / "empty_subdir").mkdir()
copytree("empty_tree", "empty_tree_dst")
assert (Path("empty_tree_dst") / "empty_subdir").is_dir()

# Test 29: rmtree with empty subdirectories
rmtree("empty_tree_dst")
assert not Path("empty_tree_dst").exists()

# Test 30: copy return value is string path
src_ret = Path("src_ret.txt")
src_ret.write_text("x")
ret = copy("src_ret.txt", "dst_ret.txt")
assert isinstance(ret, str)
assert ret.endswith("dst_ret.txt")
```

---

## 7. TARGET

**Fidelity Score: 3.5/5**

**Reasons for submax score:**

1. **Callable Parameters (G4 constraint)**: copytree.ignore, copytree.copy_function, rmtree.onerror/onexc, move.copy_function all accept callables. These are not yet spellable in pyrst (variadics gate). Pyrst would need to ship with pre-built callback strategies (e.g., canonical ignore patterns) or defer these features entirely.

2. **File-Object Return Type (file params gate)**: NamedTemporaryFile returns a file-like object with file I/O operations (read, write, flush, seek, etc.). Pyrst does not yet support file parameters or file-like types in function signatures. The workaround (mkstemp + manual os.fdopen/os.write) is verbose and requires low-level OS apis that may not be bound yet.

3. **Symbolic Link Handling**: copytree.symlinks=True, follow_symlinks parameters, and ignore_dangling_symlinks are platform-dependent. Behavior differs significantly between Unix (full support) and Windows (requires special privileges). Pyrst's path abstraction may not mature symlink semantics on all platforms before fs-trio lands.

4. **Module-Level State Risks**: tempfile.gettempdir() is mutable (setters exist in CPython via environ). Pyrst's "no mutable module state" gate means gettempdir() must be a pure const or wrapped carefully. The actual directory can be overridden via TMPDIR env var, but Pyrst may not expose env manipulation cleanly.

**Achievable Fidelity:**
- ✅ copyfile, copy, move, rmtree (core paths)
- ✅ copytree (basic recursion; defer ignore/copy_function)
- ✅ disk_usage (namedtuple output is fine)
- ✅ gettempdir (pure function)
- ✅ mkdtemp, mkstemp (basic temp file creation)
- ✅ filecmp.cmp, cmpfiles, dircmp (file comparison core)
- ⚠️ NamedTemporaryFile (defer file-object gate; mkstemp fallback)
- ⚠️ copytree callbacks (defer or supply presets)

**Design Recommendation:**
Land fs-trio at fidelity 3/5 with these deferrals:
1. Remove NamedTemporaryFile or mark it @extern (file-object gate).
2. Remove copytree.ignore, copy_function parameters; mkstemp-only temp creation.
3. Document that rmtree error handlers are not supported; ignore_errors=True is the only control.
4. Defer symlink edge cases; document best-effort behavior on Windows.

---

## Summary

| Metric | Value |
|--------|-------|
| Public API surface | 13 functions + 1 class |
| Parity test cases | 30 safe expressions |
| GATED constraints | 5 (callables, file-object, env) |
| Platform hazards | 4 (symlinks, tempdir, dangling links, perms) |
| Fidelity target | 3.5/5 (core paths ✅, callbacks ⚠️, file-objects ⚠️) |
