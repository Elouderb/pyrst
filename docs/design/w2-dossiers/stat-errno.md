# Implementation Dossier: stat-errno

## 1. SURFACE

### stat module (47 API items)

#### S_IS* Predicates (10 functions, mode → bool)
All take an unsigned int mode and return bool.

| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| S_ISREG | fn | (mode: int) -> bool | bool | True if mode is regular file (S_IFREG) |
| S_ISDIR | fn | (mode: int) -> bool | bool | True if mode is directory (S_IFDIR) |
| S_ISLNK | fn | (mode: int) -> bool | bool | True if mode is symlink (S_IFLNK) |
| S_ISFIFO | fn | (mode: int) -> bool | bool | True if mode is FIFO/pipe (S_IFIFO) |
| S_ISSOCK | fn | (mode: int) -> bool | bool | True if mode is socket (S_IFSOCK) |
| S_ISCHR | fn | (mode: int) -> bool | bool | True if mode is character device (S_IFCHR) |
| S_ISBLK | fn | (mode: int) -> bool | bool | True if mode is block device (S_IFBLK) |
| S_ISDOOR | fn | (mode: int) -> bool | bool | True if mode is door (Solaris, always False on Linux) |
| S_ISPORT | fn | (mode: int) -> bool | bool | True if mode is event port (Solaris, always False on Linux) |
| S_ISWHT | fn | (mode: int) -> bool | bool | True if mode is whiteout (BSD, always False on Linux) |

#### Mode Analysis Functions (2 functions)
| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| S_IFMT | fn | (mode: int) -> int | int | Extract file type bits (high 4 bits); result in octal 0o*0000 |
| S_IMODE | fn | (mode: int) -> int | int | Extract permission bits (low 12 bits); result in octal 0o**** |

#### ST_* Tuple Index Constants (10 const, int)
stat result tuple indices for os.stat()/lstat() result fields.

| Name | Value | Semantics |
|------|-------|-----------|
| ST_MODE | 0 | Index of st_mode (file type + permissions) |
| ST_INO | 1 | Index of st_ino (inode number) |
| ST_DEV | 2 | Index of st_dev (device ID) |
| ST_NLINK | 3 | Index of st_nlink (link count) |
| ST_UID | 4 | Index of st_uid (user ID) |
| ST_GID | 5 | Index of st_gid (group ID) |
| ST_SIZE | 6 | Index of st_size (bytes) |
| ST_ATIME | 7 | Index of st_atime (access time, Unix epoch) |
| ST_MTIME | 8 | Index of st_mtime (modification time, Unix epoch) |
| ST_CTIME | 9 | Index of st_ctime (status change time, Unix epoch) |

#### S_I* File Type Constants (10 const, int, octal values)
High bits of st_mode; use S_IFMT() to extract from full mode.

| Name | Value | Semantics |
|------|-------|-----------|
| S_IFREG | 0o100000 | Regular file |
| S_IFDIR | 0o40000 | Directory |
| S_IFLNK | 0o120000 | Symbolic link |
| S_IFIFO | 0o10000 | FIFO/named pipe |
| S_IFSOCK | 0o140000 | Socket |
| S_IFCHR | 0o20000 | Character device |
| S_IFBLK | 0o60000 | Block device |
| S_IFDOOR | 0o0 | Door (Solaris, unused on Linux) |
| S_IFPORT | 0o0 | Event port (Solaris, unused on Linux) |
| S_IFWHT | 0o0 | Whiteout (BSD, unused on Linux) |

#### S_I* Permission Constants (12 const, int, octal values)
Low bits of st_mode; permission bits (rwx for user/group/other).

| Name | Value | Semantics |
|------|-------|-----------|
| S_ISUID | 0o4000 | Set-user-ID on exec (setuid bit) |
| S_ISGID | 0o2000 | Set-group-ID on exec (setgid bit, same as S_ENFMT) |
| S_ISVTX | 0o1000 | Sticky bit (restricted deletion) |
| S_IRUSR | 0o400 | User read (owner can read) |
| S_IWUSR | 0o200 | User write (owner can write) |
| S_IXUSR | 0o100 | User exec (owner can execute) |
| S_IRGRP | 0o40 | Group read |
| S_IWGRP | 0o20 | Group write |
| S_IXGRP | 0o10 | Group execute |
| S_IROTH | 0o4 | Other read |
| S_IWOTH | 0o2 | Other write |
| S_IXOTH | 0o1 | Other execute |

#### S_I* Permission Masks (3 const, int, octal values)
Convenience masks combining rwx for each class.

| Name | Value | Semantics |
|------|-------|-----------|
| S_IRWXU | 0o700 | User rwx (S_IRUSR \| S_IWUSR \| S_IXUSR) |
| S_IRWXG | 0o70 | Group rwx (S_IRGRP \| S_IWGRP \| S_IXGRP) |
| S_IRWXO | 0o7 | Other rwx (S_IROTH \| S_IWOTH \| S_IXOTH) |

#### Deprecated Aliases (4 const, int, octal values)
Legacy names; identical to modern names.

| Name | Alias | Value |
|------|-------|-------|
| S_IREAD | S_IRUSR | 0o400 |
| S_IWRITE | S_IWUSR | 0o200 |
| S_IEXEC | S_IXUSR | 0o100 |
| S_ENFMT | S_ISGID | 0o2000 |

---

### errno module (134 API items)

#### Symbolic Error Constants (133 const, int)
POSIX and Linux error codes. All are positive integers.

| Name | Value | POSIX? | Semantics |
|------|-------|--------|-----------|
| EPERM | 1 | Yes | Operation not permitted |
| ENOENT | 2 | Yes | No such file or directory |
| ESRCH | 3 | Yes | No such process |
| EINTR | 4 | Yes | Interrupted system call |
| EIO | 5 | Yes | I/O error |
| ENXIO | 6 | Yes | No such device or address |
| E2BIG | 7 | Yes | Argument list too long |
| ENOEXEC | 8 | Yes | Exec format error |
| EBADF | 9 | Yes | Bad file descriptor |
| ECHILD | 10 | Yes | No child processes |
| EAGAIN | 11 | Yes | Resource temporarily unavailable |
| EWOULDBLOCK | 11 | Yes | Resource would block (alias for EAGAIN) |
| ENOMEM | 12 | Yes | Out of memory |
| EACCES | 13 | Yes | Permission denied |
| EFAULT | 14 | Yes | Bad address |
| ENOTBLK | 15 | No | Block device required |
| EBUSY | 16 | Yes | Device or resource busy |
| EEXIST | 17 | Yes | File exists |
| EXDEV | 18 | Yes | Invalid cross-device link |
| ENODEV | 19 | Yes | No such device |
| ENOTDIR | 20 | Yes | Not a directory |
| EISDIR | 21 | Yes | Is a directory |
| EINVAL | 22 | Yes | Invalid argument |
| ENFILE | 23 | Yes | File table overflow |
| EMFILE | 24 | Yes | Too many open files |
| ENOTTY | 25 | Yes | Not a terminal |
| ETXTBSY | 26 | No | Text file busy |
| EFBIG | 27 | Yes | File too large |
| ENOSPC | 28 | Yes | No space left on device |
| ESPIPE | 29 | Yes | Illegal seek |
| EROFS | 30 | Yes | Read-only file system |
| EMLINK | 31 | Yes | Too many links |
| EPIPE | 32 | Yes | Broken pipe |
| EDOM | 33 | Yes | Numerical argument out of domain |
| ERANGE | 34 | Yes | Numerical result out of range |
| EDEADLK | 35 | Yes | Resource deadlock avoided |
| EDEADLOCK | 35 | No | Resource deadlock avoided (alias for EDEADLK) |
| ENAMETOOLONG | 36 | Yes | File name too long |
| ENOLCK | 37 | Yes | No locks available |
| ENOSYS | 38 | Yes | Function not implemented |
| ENOTEMPTY | 39 | No | Directory not empty |
| ELOOP | 40 | Yes | Too many symbolic links |
| ENOMSG | 42 | Yes | No message of desired type |
| EIDRM | 43 | Yes | Identifier removed |
| ECHRNG | 44 | No | Channel number out of range |
| EL2NSYNC | 45 | No | Level 2 not synchronized |
| EL3HLT | 46 | No | Level 3 halted |
| EL3RST | 47 | No | Level 3 reset |
| ELNRNG | 48 | No | Link number out of range |
| EUNATCH | 49 | No | Protocol driver not attached |
| ENOCSI | 50 | No | No CSI structure available |
| EL2HLT | 51 | No | Level 2 halted |
| EBADE | 52 | No | Invalid exchange |
| EBADR | 53 | No | Invalid request descriptor |
| EXFULL | 54 | No | Exchange full |
| ENOANO | 55 | No | No anode |
| EBADRQC | 56 | No | Invalid request code |
| EBADSLT | 57 | No | Invalid slot |
| EBFONT | 59 | No | Bad font file format |
| ENOSTR | 60 | No | Device not a stream |
| ENODATA | 61 | No | No data available |
| ETIME | 62 | No | Timer expired |
| ENOSR | 63 | No | Out of streams resources |
| ENONET | 64 | No | Machine not on the network |
| ENOPKG | 65 | No | Package not installed |
| EREMOTE | 66 | No | Object is remote |
| ENOLINK | 67 | Yes | Link has been severed |
| EADV | 68 | No | Advertise error |
| ESRMNT | 69 | No | Srmount error |
| ECOMM | 70 | No | Communication error on send |
| EPROTO | 71 | Yes | Protocol error |
| EMULTIHOP | 72 | Yes | Multihop attempted |
| EDOTDOT | 73 | No | RFS specific error |
| EBADMSG | 74 | Yes | Bad message |
| EOVERFLOW | 75 | Yes | Value too large for defined data type |
| ENOTUNIQ | 76 | No | Name not unique on network |
| EBADFD | 77 | No | File descriptor in bad state |
| EREMCHG | 78 | No | Remote address changed |
| ELIBACC | 79 | No | Can not access a needed shared library |
| ELIBBAD | 80 | No | Accessing a corrupted shared library |
| ELIBSCN | 81 | No | .lib section in a.out corrupted |
| ELIBMAX | 82 | No | Attempting to link in too many shared libraries |
| ELIBEXEC | 83 | No | Cannot exec a shared library directly |
| EILSEQ | 84 | Yes | Invalid or incomplete multibyte sequence |
| ERESTART | 85 | No | Interrupted system call should be restarted |
| ESTRPIPE | 86 | No | Streams pipe error |
| EUSERS | 87 | Yes | Too many users |
| ENOTSOCK | 88 | Yes | Socket operation on non-socket |
| EDESTADDRREQ | 89 | Yes | Destination address required |
| EMSGSIZE | 90 | Yes | Message too long |
| EPROTOTYPE | 91 | Yes | Protocol wrong type for socket |
| ENOPROTOOPT | 92 | Yes | Protocol not available |
| EPROTONOSUPPORT | 93 | Yes | Protocol not supported |
| ESOCKTNOSUPPORT | 94 | Yes | Socket type not supported |
| ENOTSUP | 95 | Yes | Operation not supported |
| EOPNOTSUPP | 95 | Yes | Operation not supported (same as ENOTSUP) |
| EPFNOSUPPORT | 96 | Yes | Protocol family not supported |
| EAFNOSUPPORT | 97 | Yes | Address family not supported |
| EADDRINUSE | 98 | Yes | Address already in use |
| EADDRNOTAVAIL | 99 | Yes | Cannot assign requested address |
| ENETDOWN | 100 | Yes | Network is down |
| ENETUNREACH | 101 | Yes | Network is unreachable |
| ENETRESET | 102 | Yes | Network dropped connection on reset |
| ECONNABORTED | 103 | Yes | Software caused connection abort |
| ECONNRESET | 104 | Yes | Connection reset by peer |
| ENOBUFS | 105 | Yes | No buffer space available |
| EISCONN | 106 | Yes | Transport endpoint is already connected |
| ENOTCONN | 107 | Yes | Transport endpoint is not connected |
| ESHUTDOWN | 108 | Yes | Cannot send after transport endpoint shutdown |
| ETOOMANYREFS | 109 | Yes | Too many references: cannot splice |
| ETIMEDOUT | 110 | Yes | Connection timed out |
| ECONNREFUSED | 111 | Yes | Connection refused |
| EHOSTDOWN | 112 | Yes | Host is down |
| EHOSTUNREACH | 113 | Yes | No route to host |
| EALREADY | 114 | Yes | Operation already in progress |
| EINPROGRESS | 115 | Yes | Operation in progress |
| ESTALE | 116 | Yes | Stale file handle |
| EUCLEAN | 117 | No | Structure needs cleaning |
| ENOTNAM | 118 | No | Not a XENIX named type file |
| ENAVAIL | 119 | No | No XENIX semaphores available |
| EISNAM | 120 | No | Is a named type file |
| EREMOTEIO | 121 | No | Remote I/O error |
| EDQUOT | 122 | Yes | Disk quota exceeded |
| ENOMEDIUM | 123 | No | No medium found |
| EMEDIUMTYPE | 124 | No | Wrong medium type |
| ECANCELED | 125 | Yes | Operation canceled |
| ENOKEY | 126 | No | Required key not available |
| EKEYEXPIRED | 127 | No | Key has expired |
| EKEYREVOKED | 128 | No | Key has been revoked |
| EKEYREJECTED | 129 | No | Key was rejected by service |
| EOWNERDEAD | 130 | Yes | Owner died (mutex/semaphore) |
| ENOTRECOVERABLE | 131 | Yes | State not recoverable |
| ERFKILL | 132 | No | Operation not possible due to RF-kill |

#### errorcode Dict (1 const, dict)
| Name | Kind | Type | Semantics |
|------|------|------|-----------|
| errorcode | const | dict[int, str] | Maps error code int to symbolic name str; length 130; access via key int (KeyError if missing) |

---

## 2. ERRORS

### S_IS* Predicates — Type Errors
All S_IS* predicates require unsigned int argument. Accepts int, raises TypeError for str/float/None, OverflowError for negative.

| Probe | Exception | Message |
|-------|-----------|---------|
| `stat.S_ISREG("123")` | TypeError | an integer is required |
| `stat.S_ISREG(1.5)` | TypeError | an integer is required |
| `stat.S_ISREG(None)` | TypeError | an integer is required |
| `stat.S_ISREG(-1)` | OverflowError | can't convert negative value to unsigned int |
| `stat.S_ISDIR(-1)` | OverflowError | can't convert negative value to unsigned int |
| `stat.S_ISLNK(-1)` | OverflowError | can't convert negative value to unsigned int |

### S_IFMT / S_IMODE — Type Errors
Same requirements: unsigned int, TypeError for non-int, OverflowError for negative.

| Probe | Exception | Message |
|-------|-----------|---------|
| `stat.S_IFMT(-1)` | OverflowError | can't convert negative value to unsigned int |

### errno.errorcode — Key Errors
Dict access via int key only. KeyError on missing int key or non-int key.

| Probe | Exception | Message |
|-------|-----------|---------|
| `errno.errorcode[9999]` | KeyError | 9999 |
| `errno.errorcode['EPERM']` | KeyError | 'EPERM' |

---

## 3. BEHAVIOR MATRIX (86 Probed Cases)

### S_IS* Predicates vs File Type Constants

```
stat.S_ISREG(stat.S_IFREG) = True
stat.S_ISREG(stat.S_IFDIR) = False
stat.S_ISDIR(stat.S_IFDIR) = True
stat.S_ISDIR(stat.S_IFREG) = False
stat.S_ISLNK(stat.S_IFLNK) = True
stat.S_ISLNK(stat.S_IFREG) = False
stat.S_ISFIFO(stat.S_IFIFO) = True
stat.S_ISFIFO(stat.S_IFREG) = False
stat.S_ISSOCK(stat.S_IFSOCK) = True
stat.S_ISSOCK(stat.S_IFREG) = False
stat.S_ISCHR(stat.S_IFCHR) = True
stat.S_ISCHR(stat.S_IFREG) = False
stat.S_ISBLK(stat.S_IFBLK) = True
stat.S_ISBLK(stat.S_IFREG) = False
stat.S_ISREG(0) = False
stat.S_ISDIR(0) = False
stat.S_ISDOOR(0) = False
stat.S_ISPORT(0) = False
stat.S_ISWHT(0) = False
```

### ST_* Constants

```
stat.ST_MODE = 0
stat.ST_INO = 1
stat.ST_DEV = 2
stat.ST_NLINK = 3
stat.ST_UID = 4
stat.ST_GID = 5
stat.ST_SIZE = 6
stat.ST_ATIME = 7
stat.ST_MTIME = 8
stat.ST_CTIME = 9
```

### File Type Constants (Octal)

```
stat.S_IFREG = 0o100000
stat.S_IFDIR = 0o40000
stat.S_IFLNK = 0o120000
stat.S_IFIFO = 0o10000
stat.S_IFSOCK = 0o140000
stat.S_IFCHR = 0o20000
stat.S_IFBLK = 0o60000
```

### Permission Bits (Octal)

```
stat.S_ISUID = 0o4000
stat.S_ISGID = 0o2000
stat.S_ISVTX = 0o1000
stat.S_IRUSR = 0o400
stat.S_IWUSR = 0o200
stat.S_IXUSR = 0o100
stat.S_IRWXU = 0o700
stat.S_IRGRP = 0o40
stat.S_IWGRP = 0o20
stat.S_IXGRP = 0o10
stat.S_IRWXG = 0o70
stat.S_IROTH = 0o4
stat.S_IWOTH = 0o2
stat.S_IXOTH = 0o1
stat.S_IRWXO = 0o7
```

### Mode Functions (S_IFMT, S_IMODE)

```
stat.S_IFMT(stat.S_IFREG | 0o644) = 0o100000
stat.S_IFMT(stat.S_IFDIR | 0o755) = 0o40000
stat.S_IMODE(stat.S_IFREG | 0o644) = 0o644
stat.S_IMODE(stat.S_IFDIR | 0o755) = 0o755
```

### Deprecated Aliases (Octal)

```
stat.S_IREAD = 0o400
stat.S_IWRITE = 0o200
stat.S_IEXEC = 0o100
stat.S_ENFMT = 0o2000
```

### errno Constants (Sample)

```
errno.EPERM = 1
errno.ENOENT = 2
errno.ESRCH = 3
errno.EINTR = 4
errno.EIO = 5
errno.EAGAIN = 11
errno.ENOMEM = 12
errno.EACCES = 13
errno.EBADF = 9
errno.EBUSY = 16
errno.EEXIST = 17
errno.EISDIR = 21
errno.EINVAL = 22
errno.ENFILE = 23
errno.EMFILE = 24
errno.EROFS = 30
errno.EPIPE = 32
errno.ERANGE = 34
errno.ENOSYS = 38
errno.EWOULDBLOCK = 11
```

### errno.errorcode Dict

```
errno.errorcode[1] = 'EPERM'
errno.errorcode[2] = 'ENOENT'
errno.errorcode[5] = 'IIO'
errno.errorcode[22] = 'EINVAL'
errno.errorcode.get(1) = 'EPERM'
errno.errorcode.get(999) = None
len(errno.errorcode) = 130
```

---

## 4. HAZARDS

### Dict Iteration Order (pyrst vs CPython)
**CRITICAL FOR PYRST:** pyrst dicts iterate sorted-key order, CPython 3.7+ uses insertion order. `errno.errorcode` is a pre-built dict mapping int → str. Keys are monotonically increasing (1, 2, 3, ..., 132). Iteration order will be identical (sorted by key = insertion order). **SAFE for pyrst**.

### Octal Literal Representation
All stat constants use octal notation in CPython (e.g., `0o100000`). Decimal equivalents: S_IFREG=32768, S_IFDIR=16384. Pyrst must preserve octal if printing, but internal int values are portable.

### No Float or String Conversions
stat predicates and mode functions strictly require unsigned int. No coercion from str/float. Negative integers raise OverflowError (not just a wrap/clamp). This is a hard boundary, no edge-case surprises.

### errno.errorcode Access Pattern
Keys are always int, values always str (the symbolic name). No reverse map (name → code) provided; users must build it if needed. .get() method available and returns None for missing keys (safe default).

### Solaris/BSD-Specific Predicates
S_ISDOOR, S_ISPORT, S_ISWHT and corresponding S_IFDOOR, S_IFPORT, S_IFWHT are always 0 or False on Linux. They are present for portability but non-functional on POSIX/Linux. Include in dossier but document as Linux-always-false.

### EAGAIN == EWOULDBLOCK
On Linux, EAGAIN (11) and EWOULDBLOCK (11) are the same constant value. Both names exist in errno module, both map to 11. errorcode dict has a single entry: `errno.errorcode[11]` = 'EAGAIN'. No ambiguity in the mapping (one key-value pair), but the name returned depends on implementation (currently 'EAGAIN' on Linux).

---

## 5. GATED

### G2: Module-Level Mutable State
**GATE HIT:** `errno.errorcode` is a module-level dict. In pyrst, module-level state must be immutable (literal consts). **DESIGN WORKAROUND:** Split into a frozen/constant dict-like view (e.g., `errorcode_map: static Dict[int, str]` populated at module init from const pairs), or expose only individual const pairs and omit the dict if mutable state is forbidden. **HONEST DEFERRAL:** If pyrst requires all module state to be immutable, errorcode dict cannot be ported as-is; either make it a frozen dict (read-only), or provide access via a function that returns the value for a given key (single-lookup only, no iteration).

### G3: No Dotted Submodules
**NOT HIT:** stat and errno are both flat modules (no stat.mode, errno.socket submodules). All constants and functions are top-level.

### G4: No *args/**kwargs Variadics
**NOT HIT:** stat predicates and mode functions take fixed single int argument. errno constants are module-level ints. No variadics used.

### G7: No bytes Type
**NOT HIT:** errno.errorcode values are str (not bytes). stat mode values are int. No bytes involved.

### G9: No Bignum (i64 Ints)
**NOT HIT:** All int values fit in signed 64-bit range. ST_* indices: 0-9. S_I* bits: max 0o140000 (65536 decimal), well under i64. errno constants: max 132. No overflow risk.

**HONEST SUMMARY:** Only gate G2 (mutable module state) applies. errorcode dict must either be frozen or accessed via immutable const pairs. Recommend providing immutable frozen dict if pyrst supports that, otherwise omit the dict and provide a function `def errorcode(code: int) -> str` that returns the symbolic name or raises KeyError.

---

## 6. PARITY PLAN (56 Dual-Run-Safe Test Cases)

All tests use only immutable operations (constant access, pure predicate calls, dict .get() for safe access). No file I/O, no locale/time dependence, no randomness. Order-safe: octal literals convert to decimal ints internally; stat constants are numeric; errno constants are numeric; errorcode keys are sorted (both CPython insertion order and pyrst sorted order match).

### stat.S_IS* Predicates (14 cases)

```python
assert stat.S_ISREG(stat.S_IFREG) == True
assert stat.S_ISDIR(stat.S_IFDIR) == True
assert stat.S_ISLNK(stat.S_IFLNK) == True
assert stat.S_ISFIFO(stat.S_IFIFO) == True
assert stat.S_ISSOCK(stat.S_IFSOCK) == True
assert stat.S_ISCHR(stat.S_IFCHR) == True
assert stat.S_ISBLK(stat.S_IFBLK) == True
assert stat.S_ISREG(stat.S_IFDIR) == False
assert stat.S_ISDIR(stat.S_IFREG) == False
assert stat.S_ISLNK(stat.S_IFREG) == False
assert stat.S_ISFIFO(stat.S_IFREG) == False
assert stat.S_ISSOCK(stat.S_IFREG) == False
assert stat.S_ISCHR(stat.S_IFREG) == False
assert stat.S_ISBLK(stat.S_IFREG) == False
```

### stat.ST_* Index Constants (10 cases)

```python
assert stat.ST_MODE == 0
assert stat.ST_INO == 1
assert stat.ST_DEV == 2
assert stat.ST_NLINK == 3
assert stat.ST_UID == 4
assert stat.ST_GID == 5
assert stat.ST_SIZE == 6
assert stat.ST_ATIME == 7
assert stat.ST_MTIME == 8
assert stat.ST_CTIME == 9
```

### stat.S_I* File Type Constants (7 cases)

```python
assert stat.S_IFREG == 0o100000
assert stat.S_IFDIR == 0o40000
assert stat.S_IFLNK == 0o120000
assert stat.S_IFIFO == 0o10000
assert stat.S_IFSOCK == 0o140000
assert stat.S_IFCHR == 0o20000
assert stat.S_IFBLK == 0o60000
```

### stat.S_I* Permission Bits (15 cases)

```python
assert stat.S_ISUID == 0o4000
assert stat.S_ISGID == 0o2000
assert stat.S_ISVTX == 0o1000
assert stat.S_IRUSR == 0o400
assert stat.S_IWUSR == 0o200
assert stat.S_IXUSR == 0o100
assert stat.S_IRWXU == 0o700
assert stat.S_IRGRP == 0o40
assert stat.S_IWGRP == 0o20
assert stat.S_IXGRP == 0o10
assert stat.S_IRWXG == 0o70
assert stat.S_IROTH == 0o4
assert stat.S_IWOTH == 0o2
assert stat.S_IXOTH == 0o1
assert stat.S_IRWXO == 0o7
```

### stat Mode Functions (4 cases)

```python
assert stat.S_IFMT(stat.S_IFREG | 0o644) == stat.S_IFREG
assert stat.S_IFMT(stat.S_IFDIR | 0o755) == stat.S_IFDIR
assert stat.S_IMODE(stat.S_IFREG | 0o644) == 0o644
assert stat.S_IMODE(stat.S_IFDIR | 0o755) == 0o755
```

### errno Constants (Common Subset, 10 cases)

```python
assert errno.EPERM == 1
assert errno.ENOENT == 2
assert errno.ESRCH == 3
assert errno.EINTR == 4
assert errno.EIO == 5
assert errno.EAGAIN == 11
assert errno.ENOMEM == 12
assert errno.EACCES == 13
assert errno.EBADF == 9
assert errno.EBUSY == 16
```

### errno.errorcode Dict Access (Safe patterns, 6 cases)

```python
assert errno.errorcode.get(1) == 'EPERM'
assert errno.errorcode.get(2) == 'ENOENT'
assert errno.errorcode.get(5) == 'EIO'
assert errno.errorcode.get(22) == 'EINVAL'
assert errno.errorcode.get(999) == None
assert len(errno.errorcode) == 130
```

---

## 7. TARGET

### Fidelity Estimate: 4/5

**Reasoning:**

1. **Clean API (stat predicates, constants, errno constants):** 90% of the surface is straightforward numeric constants and simple predicates taking unsigned int. Easily portable to pyrst. errno symbolic constants are module-level immutable ints; ST_* and S_I* constants are immutable ints. No surprises. **Impact: +1 toward 5/5**

2. **Mutable Dict (errorcode):** errno.errorcode is a live dict that pyrst's immutable-module-state rule forbids. Must be frozen or converted to const pairs / accessor function. This is a design gate, not a semantic complexity. **Impact: -0.5 from 5/5**

3. **Platform-Specific Constants (Solaris/BSD Predicates):** S_ISDOOR, S_ISPORT, S_ISWHT and related file type bits are always 0/False on Linux. Pyrst can expose them as consts (always 0) or skip them for Linux-only codegen. Minor complication, easily documented. **Impact: -0.25 from 4.5/5**

4. **Semantic Fidelity:** S_IS* predicates are pure unsigned-int → bool (no side effects, no state). S_IFMT and S_IMODE are pure int → int. errno constants are immutable. No locale, time, or platform variability (beyond the Solaris bits). Full parity achievable. **Impact: no further loss**

**Final Score: 4/5**

**Why Not 5/5:**

1. **Gate G2 (errorcode dict mutability):** Requires design decision (freeze, skip, or accessor function) before porting. Not a semantic issue, but a structural one.
2. **Minor Solaris/BSD platform specifics:** S_IFDOOR, S_ISPORT, S_ISWHT will always be 0 on target platform (Linux). Correct behavior, but non-functional APIs add surface area without real use.

**Dominant Reasons:**
- errorcode dict must be immutable (architectural constraint)
- Platform-specific predicates add surface area; minimal real-world use on Linux
- All other semantics are 100% portable (predicates, bit constants, errno values)
