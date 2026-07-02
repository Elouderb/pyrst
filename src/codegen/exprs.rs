use super::*;

impl<'a> Codegen<'a> {
    /// (card cc7ae370, item 1) Thin wrapper: collects any subscript-index temps
    /// hoisted for a MUTATING-method receiver (`grid[len(grid) - 1].append(x)`)
    /// and wraps the whole emitted call in a `{ let __idxN = ..; <call> }` block so
    /// the index temp runs before the receiver's `&mut` borrow (E0502). Wrapping
    /// the ENTIRE call — with the bare receiver place still INSIDE — rather than
    /// just the receiver keeps two-phase borrows valid for the method's own
    /// arguments (`grid[i].append(grid[j])`). With no subscript receiver the
    /// prelude is empty and the call is returned unchanged (byte-identical).
    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_method_call_on_attr(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
    ) -> Result<Option<String>> {
        let mut recv_prelude = Vec::new();
        let result = self.emit_method_call_on_attr_inner(callee, args, &mut recv_prelude)?;
        Ok(result.map(|s| Self::hoist_wrap(&recv_prelude, s)))
    }

    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_method_call_on_attr_inner(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        recv_prelude: &mut Vec<String>,
    ) -> Result<Option<String>> {
                // Method call with attribute callee — handle method name remapping
                if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    // Qualified module call `X.f(args)` for a REAL imported module
                    // (card 81db88e0). When X is a tracked module name and f is one
                    // of its functions, lower the call to the FLAT function `f(args)`
                    // — every imported module's functions are merged into `ctx.funcs`
                    // under their bare name, so the flat call resolves at codegen and
                    // build. We re-dispatch through `emit_call` with a synthesized
                    // `Ident(f)` callee so the regular function-call machinery
                    // (Optional/by-ref/default-argument coercion) applies uniformly,
                    // exactly as if the user had written `from X import f; f(args)`.
                    // `math` is now a REAL embedded module (`lib/math.pyrs`), so
                    // `math.sqrt(x)` flows through here too (its @extern `sqrt`
                    // is merged into `module_funcs`/`ctx.funcs`); the former
                    // hardcoded math call-arm is gone. We re-dispatch through
                    // `emit_plain_func_call` (NOT `emit_call`) so a module
                    // function whose flat name COLLIDES with a builtin — e.g.
                    // `math.pow` vs the builtin `pow` — calls the MODULE function,
                    // not the builtin int-pow. NOTE: flat emission means a
                    // cross-module same-name collision between two modules is
                    // unresolved (stdlib uses distinct names; per-module
                    // namespacing `X__f` is a later refinement).
                    if let Expr::Ident(modname, _) = obj.as_ref() {
                        if self.ctx.module_funcs.get(modname).is_some_and(|fns| fns.iter().any(|n| n == name)) {
                            let span = callee.span();
                            let flat_callee: Box<Expr> = Box::new(Expr::Ident(name.clone(), span));
                            return Ok(Some(self.emit_plain_func_call(&flat_callee, args, &[])?));
                        }
                    }

                    // Check for static method calls: ClassName.method(args)
                    if let Expr::Ident(class_name, _) = obj.as_ref() {
                        if let Some(class_def) = self.ctx.classes.get(class_name.as_str()) {
                            if let Some(method_def) = class_def.methods.iter().find(|m| &m.name == name) {
                                if method_def.decorators.contains(&"staticmethod".to_string()) {
                                    let parts: Result<Vec<_>> = args.iter().map(|a| self.emit_consuming(a)).collect();
                                    let parts = parts?;
                                    return Ok(Some(format!("{}::{}({})", class_name, name, parts.join(", "))));
                                }
                            }
                        }
                    }

                    // Mutating list/set/dict methods need an lvalue receiver. For
                    // a *subscripted* receiver (`self.rows[i].append(x)`,
                    // `grid[r].sort()`) emit_expr would produce a clone-based
                    // rvalue, so the mutation would hit (and drop) a temporary.
                    // Use emit_place for those so the in-place mutation lands on
                    // the real element. Bare-name and `self.field` receivers are
                    // already place expressions under emit_expr.
                    // MUTATING_METHODS is the module-level const above.
                    // (card cc7ae370, item 1) Hoist the subscript receiver's index
                    // temps into `recv_prelude`; the wrapper blocks the whole call
                    // so the temp runs before the `&mut` receiver borrow (E0502).
                    // The receiver stays a bare place here, preserving two-phase
                    // borrows for the method's arguments.
                    let obj_s = if matches!(obj.as_ref(), Expr::Index { .. })
                        && MUTATING_METHODS.contains(&name.as_str())
                    {
                        self.emit_place_hoisted(obj, recv_prelude)?
                    } else {
                        self.emit_expr(obj)?
                    };
                    let method = match name.as_str() {
                        // String methods
                        "upper"      => "to_uppercase",
                        "lower"      => "to_lowercase",
                        "strip"      => "trim",
                        "lstrip"     => "trim_start",
                        "rstrip"     => "trim_end",
                        // List methods
                        "append"     => "push",
                        "pop"        => "pop().unwrap",
                        // passthrough
                        other        => other,
                    };
                    let parts: Result<Vec<_>> = args.iter().map(|a| self.emit_consuming(a)).collect();
                    let parts = parts?;

                    // (EPIC-6) Receiver-type-guarded early return. The builtin
                    // method arms below match purely on `name` with NO receiver
                    // guard on most of them (`get`, `keys`, `values`, `items`,
                    // `update`, `pop`, `copy`, `clear`, `append`, `extend`,
                    // `insert`, `remove`, `sort`, ...). So a USER class that
                    // defines a method with one of those names previously had
                    // `instance.get(k)` silently lowered to a dict
                    // `.get(&k).cloned()` (wrong Rust / wrong behavior / a
                    // compile error) — the builtin arm won because it ran BEFORE
                    // the user-method tail. Guard it here: if the receiver's
                    // static type is a user class that HAS an instance method
                    // named `name` (resolved via `get_method`, walking the
                    // inheritance chain — the SAME lookup the user-method tail
                    // uses), dispatch to that user method NOW and return,
                    // bypassing every builtin arm. A builtin receiver
                    // (str/list/dict/set/file) is never `Ty::Class`, so the
                    // guard never fires for it and the builtin arms below run
                    // byte-for-byte unchanged. A polymorphic-base receiver
                    // composes too: `cls` is the base name, `get_method` returns
                    // the base's signature, and `obj_s.name(..)` resolves to the
                    // companion enum `cls__`'s dispatch method — identical to the
                    // pre-existing EPIC-5 lowering.
                    if let Ty::Class(cls, _) = self.type_of_expr(obj.as_ref()) {
                        if self.ctx.get_method(&cls, name).is_some() {
                            return self.emit_user_method_call(&obj_s, &cls, name, args, &parts).map(Some);
                        }
                        // Not a method — a `Callable` FIELD invoked as `obj.f(args)`.
                        // The field holds an `Rc<dyn Fn(..) -> ..>`, so Rust needs the
                        // field-access parenthesised before the call: `(obj.f)(args)`.
                        // (`obj.f(args)` would be parsed as a method named `f`, which
                        // does not exist — E0599.) Resolved via the same field-type
                        // lookup used elsewhere so it walks the inheritance chain.
                        if let Some(field_ty) = self
                            .ctx
                            .classes
                            .get(cls.as_str())
                            .and_then(|cd| self.class_field_type(cd, name))
                        {
                            if matches!(field_ty, Ty::Func(..)) {
                                return Ok(Some(format!(
                                    "({}.{})({})",
                                    obj_s,
                                    escape_ident(name),
                                    parts.join(", ")
                                )));
                            }
                        }
                    }

                    // Special handling for string methods that return &str and need to be converted to String
                    if matches!(name.as_str(), "strip" | "lstrip" | "rstrip") {
                        return Ok(Some(format!("{}.{}().to_string()", obj_s, method)));
                    }

                    // Special case: split()
                    if name == "split" {
                        return if args.is_empty() {
                            Ok(Some(format!("{}.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>()", obj_s)))
                        } else {
                            let sep = parts[0].clone();
                            Ok(Some(format!("{}.split({}.as_str()).map(|s| s.to_string()).collect::<Vec<_>>()", obj_s, sep)))
                        };
                    }

                    // Special case: join()
                    if name == "join" {
                        return Ok(Some(format!("{}.join(&{})", parts[0], obj_s)));
                    }

                    // Special case: len() as method
                    if name == "len" {
                        // str length is character count, not UTF-8 byte count.
                        if matches!(self.type_of_expr(obj.as_ref()), Ty::Str) {
                            return Ok(Some(format!("{}.chars().count() as i64", obj_s)));
                        }
                        return Ok(Some(format!("{}.len() as i64", obj_s)));
                    }

                    // Special case: get() for dicts. Arg-count-aware, mirroring
                    // the static typing in `typeck::dict_get_ret`:
                    //   d.get(k)           -> Option<V>  (None when absent), so a
                    //                         caller can narrow it with `is None`.
                    //   d.get(k, default)  -> V          (the supplied fallback).
                    if name == "get" {
                        // A user-class receiver with a `get` method has already been
                        // dispatched above; reaching here means a dict `.get()`. It
                        // requires at least the key argument — a no-arg `.get()` is an
                        // honest error (NEVER an index-out-of-bounds panic on parts[0]).
                        if parts.is_empty() {
                            return Err(crate::diag::Error::Codegen(
                                "`.get()` requires a key argument (dict.get(k) or dict.get(k, default))".into(),
                            ));
                        }
                        if parts.len() > 1 {
                            return Ok(Some(format!(
                                "{}.get(&{}).cloned().unwrap_or({})",
                                obj_s, parts[0], parts[1]
                            )));
                        }
                        return Ok(Some(format!("{}.get(&{}).cloned()", obj_s, parts[0])));
                    }

                    // String methods
                    if name == "startswith" && !parts.is_empty() {
                        return Ok(Some(format!("{}.starts_with({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "endswith" && !parts.is_empty() {
                        return Ok(Some(format!("{}.ends_with({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "replace" && parts.len() >= 2 {
                        return Ok(Some(format!("{}.replace({}.as_str(), {}.as_str())", obj_s, parts[0], parts[1])));
                    }
                    if name == "removeprefix" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __prefix = {}; \
                            if __s.starts_with(__prefix.as_str()) {{ __s[__prefix.len()..].to_string() }} else {{ __s }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "removesuffix" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __suffix = {}; \
                            if __s.ends_with(__suffix.as_str()) {{ __s[..__s.len() - __suffix.len()].to_string() }} else {{ __s }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "expandtabs" {
                        let tab_size = if !parts.is_empty() {
                            format!("{} as usize", parts[0])
                        } else {
                            "8usize".to_string()
                        };
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __tab_size = {}; \
                            __s.replace('\\t', &\" \".repeat(__tab_size)) }}",
                            obj_s, tab_size
                        )));
                    }
                    if name == "partition" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __sep = {}; \
                            if let Some(__idx) = __s.find(__sep.as_str()) {{ \
                            vec![__s[..__idx].to_string(), __sep.clone(), __s[__idx + __sep.len()..].to_string()] \
                            }} else {{ vec![__s, String::new(), String::new()] }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "rpartition" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __sep = {}; \
                            if let Some(__idx) = __s.rfind(__sep.as_str()) {{ \
                            vec![__s[..__idx].to_string(), __sep.clone(), __s[__idx + __sep.len()..].to_string()] \
                            }} else {{ vec![String::new(), String::new(), __s] }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "find" && !parts.is_empty() {
                        return Ok(Some(format!("{}.find({}.as_str()).map(|i| i as i64).unwrap_or(-1i64)", obj_s, parts[0])));
                    }
                    if name == "contains" && !parts.is_empty() {
                        return Ok(Some(format!("{}.contains({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "rfind" && !parts.is_empty() {
                        return Ok(Some(format!("{}.rfind({}.as_str()).map(|i| i as i64).unwrap_or(-1i64)", obj_s, parts[0])));
                    }
                    if name == "rindex" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __idx = {}.rfind({}.as_str()); match __idx {{ Some(i) => i as i64, None => panic!(\"ValueError\\0substring not found\") }} }}",
                            obj_s, parts[0]
                        )));
                    }

                    // String utility methods
                    if name == "isdigit" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_numeric()))", obj_s, obj_s)));
                    }
                    if name == "isalpha" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_alphabetic()))", obj_s, obj_s)));
                    }
                    if name == "isupper" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) && {}.chars().any(|c| c.is_alphabetic()))", obj_s, obj_s, obj_s)));
                    }
                    if name == "islower" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_lowercase()) && {}.chars().any(|c| c.is_alphabetic()))", obj_s, obj_s, obj_s)));
                    }
                    if name == "isspace" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_whitespace()))", obj_s, obj_s)));
                    }
                    if name == "isalnum" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_alphanumeric()))", obj_s, obj_s)));
                    }
                    if name == "isidentifier" {
                        return Ok(Some(format!(
                            "(!{}.is_empty() && ({}.chars().next().unwrap().is_alphabetic() || {}.chars().next().unwrap() == '_') && {}.chars().all(|c| c.is_alphanumeric() || c == '_'))",
                            obj_s, obj_s, obj_s, obj_s
                        )));
                    }
                    if name == "isnumeric" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_numeric()))", obj_s, obj_s)));
                    }
                    if name == "isprintable" {
                        return Ok(Some(format!("({}.chars().all(|c| !c.is_control()))", obj_s)));
                    }
                    if name == "istitle" {
                        return Ok(Some(format!(
                            "(!{}.is_empty() && {}.split_whitespace().all(|word| if word.is_empty() {{ true }} else {{ word.chars().next().unwrap().is_uppercase() && word[1..].chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) }}))",
                            obj_s, obj_s
                        )));
                    }

                    // Additional string methods
                    if name == "capitalize" {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); if __s.is_empty() {{ __s }} else {{ format!(\"{{}}{{}}\" , __s.chars().next().unwrap().to_uppercase(), &__s[1..].to_lowercase()) }} }}",
                            obj_s
                        )));
                    }
                    if name == "title" {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); __s.split_whitespace().map(|w| if w.is_empty() {{ w.to_string() }} else {{ format!(\"{{}}{{}}\" , w.chars().next().unwrap().to_uppercase(), &w[1..].to_lowercase()) }} ).collect::<Vec<_>>().join(\" \") }}",
                            obj_s
                        )));
                    }
                    if name == "zfill" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:0>width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "ljust" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:<width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "rjust" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:>width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "center" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ let __total = __width - __s.len(); let __left = (__total + 1) / 2; let __right = __total / 2; format!(\"{{}}{{}}{{}}\" , \" \".repeat(__left), __s, \" \".repeat(__right)) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "swapcase" {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); __s.chars().map(|c| if c.is_uppercase() {{ c.to_lowercase().to_string() }} else {{ c.to_uppercase().to_string() }} ).collect::<String>() }}",
                            obj_s
                        )));
                    }
                    if name == "splitlines" {
                        return Ok(Some(format!(
                            "{}.lines().map(|l| l.to_string()).collect::<Vec<_>>()",
                            obj_s
                        )));
                    }
                    if name == "count" && !parts.is_empty() {
                        let obj_ty = self.type_of_expr(obj);
                        match obj_ty {
                            Ty::Str => {
                                return Ok(Some(format!(
                                    "{{ let __s = {}.clone(); let __sub = {}; let mut __count = 0i64; let mut __start = 0; while let Some(__pos) = __s.as_str()[__start..].find(__sub.as_str()) {{ __count += 1; __start += __pos + __sub.len(); }} __count }}",
                                    obj_s, parts[0]
                                )));
                            }
                            _ => {} // Fall through to list count below
                        }
                    }
                    if name == "index" && !parts.is_empty() {
                        let obj_ty = self.type_of_expr(obj);
                        match obj_ty {
                            Ty::Str => {
                                return Ok(Some(format!(
                                    "{}.find({}.as_str()).map(|i| i as i64).unwrap_or_else(|| panic!(\"ValueError\\0substring not found\"))",
                                    obj_s, parts[0]
                                )));
                            }
                            _ => {} // Fall through to list index below
                        }
                    }

                    // File methods (PyFile; gated on a File receiver). write takes
                    // &str, so borrow the argument.
                    if let Ty::File = self.type_of_expr(obj) {
                        match name.as_str() {
                            "write" if !parts.is_empty() => return Ok(Some(format!("{}.write(&{})", obj_s, parts[0]))),
                            "write" => return Err(crate::diag::Error::Codegen("file write() requires one argument".into())),
                            "read" | "readlines" | "close" =>
                                return Ok(Some(format!("{}.{}()", obj_s, name))),
                            _ => {}
                        }
                    }

                    // Dict views - materialize into a Vec so they work both in a
                    // for-loop and as a value (e.g. print(d.keys()), len(d.values())),
                    // matching their List(K)/List(V) static type.
                    if name == "keys" {
                        return Ok(Some(format!("{}.keys().cloned().collect::<Vec<_>>()", obj_s)));
                    }
                    if name == "values" {
                        return Ok(Some(format!("{}.values().cloned().collect::<Vec<_>>()", obj_s)));
                    }
                    if name == "items" {
                        // Collect into a Vec<(K, V)> so the for-loop lowering treats it
                        // as a normal collection (it wraps the iterable in .iter().cloned()).
                        return Ok(Some(format!("{}.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>()", obj_s)));
                    }

                    // Set methods (gated on receiver type — many names overlap with
                    // list/dict, so disambiguate by the static type of the receiver).
                    if let Ty::Set(_) = self.type_of_expr(obj) {
                        match name.as_str() {
                            // insert takes ownership, so emit the element owned
                            // (a String var becomes `x.clone()`).
                            "add" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.insert({}); }}", obj_s, self.emit_consuming(&args[0])?))),
                            // NB: unlike Python, neither discard nor remove raises on an
                            // absent element here (Rust's HashSet::remove returns an ignored bool).
                            "discard" | "remove" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.remove(&{}); }}", obj_s, parts[0]))),
                            "update" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.extend({}.iter().cloned()); }}", obj_s, parts[0]))),
                            "union" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.union(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "intersection" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.intersection(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "difference" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.difference(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "symmetric_difference" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.symmetric_difference(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "issubset" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_subset(&{})", obj_s, parts[0]))),
                            "issuperset" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_superset(&{})", obj_s, parts[0]))),
                            "isdisjoint" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_disjoint(&{})", obj_s, parts[0]))),
                            _ => {}
                        }
                    }

                    // dict.update(other) — merge another mapping in place.
                    if name == "update" && !parts.is_empty() {
                        return Ok(Some(format!("{{ {}.extend({}); }}", obj_s, parts[0])));
                    }

                    if name == "pop" {
                        // list.pop(): remove and return the last element (or pop(i) -> remove index).
                        if let Ty::List(_) = self.type_of_expr(obj) {
                            return Ok(Some(if parts.is_empty() {
                                format!("{}.pop().unwrap_or_else(|| panic!(\"IndexError\\0pop from empty list\"))", obj_s)
                            } else {
                                // Honor Python negative indices: pop(-1) is the last element.
                                format!(
                                    "{{ let __n = {obj}.len() as i64; let __i = {idx}; \
                                     {obj}.remove((if __i < 0 {{ __n + __i }} else {{ __i }}) as usize) }}",
                                    obj = obj_s, idx = parts[0]
                                )
                            }));
                        }
                        // dict.pop(key[, default])
                        if parts.is_empty() {
                            return Err(crate::diag::Error::Codegen("pop requires at least one argument".into()));
                        } else if parts.len() == 1 {
                            // pop(key) — remove from the receiver and return the value (panic if absent)
                            return Ok(Some(format!("{}.remove(&{}).unwrap_or_else(|| panic!(\"KeyError\\0<key>\"))", obj_s, parts[0])));
                        } else {
                            // pop(key, default) — remove from the receiver; default if absent
                            return Ok(Some(format!("{}.remove(&{}).unwrap_or({})", obj_s, parts[0], parts[1])));
                        }
                    }
                    // List methods
                    if name == "extend" && !parts.is_empty() {
                        return Ok(Some(format!("{}.extend({})", obj_s, parts[0])));
                    }
                    if name == "insert" && parts.len() >= 2 {
                        return Ok(Some(format!("{}.insert({} as usize, {})", obj_s, parts[0], parts[1])));
                    }
                    if name == "remove" && !parts.is_empty() {
                        return Ok(Some(format!("{{ let __idx = {}.iter().position(|__x| *__x == {}).unwrap_or_else(|| panic!(\"ValueError\\0value not found\")); {}.remove(__idx); }}", obj_s, parts[0], obj_s)));
                    }
                    if name == "index" && !parts.is_empty() {
                        return Ok(Some(format!("{}.iter().position(|__x| *__x == {}).unwrap_or_else(|| panic!(\"ValueError\\0value not found\")) as i64", obj_s, parts[0])));
                    }
                    if name == "count" && !parts.is_empty() {
                        return Ok(Some(format!("{}.iter().filter(|__x| **__x == {}).count() as i64", obj_s, parts[0])));
                    }
                    if name == "reverse" {
                        return Ok(Some(format!("{}.reverse()", obj_s)));
                    }
                    if name == "sort" {
                        return Ok(Some(format!("{}.sort()", obj_s)));
                    }
                    if name == "clear" {
                        return Ok(Some(format!("{}.clear()", obj_s)));
                    }
                    if name == "copy" {
                        return Ok(Some(format!("{}.clone()", obj_s)));
                    }

                    // Regular method call.
                    // (EPIC-4 V2-c) Thread `&mut <place>` for any by-reference
                    // (`Mut[T]`) method parameter so the callee's mutation persists
                    // to the caller. The method's per-param by-ref flags come from
                    // get_method (self-EXCLUSIVE and index-aligned to `args` after
                    // STEP 0). Only user-defined methods on a known class receiver
                    // can be by-ref; the builtin string/list/dict branches above
                    // all `return`ed earlier, so the by-value `parts` they share is
                    // never reached here. We rebuild `parts` only when the receiver
                    // resolves to a class with a matching method that actually has
                    // a by-ref param; otherwise the original by-value `parts`
                    // (clone-on-use) is used unchanged.
                    let method_by_ref: Vec<bool> =
                        if let Ty::Class(cls, _) = self.type_of_expr(obj.as_ref()) {
                            self.ctx.get_method(&cls, name)
                                .map(|sig| sig.param_by_ref.clone())
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                    if method_by_ref.iter().any(|&b| b) {
                        let mut mparts = Vec::with_capacity(args.len());
                        for (i, a) in args.iter().enumerate() {
                            if method_by_ref.get(i).copied().unwrap_or(false) {
                                // (card cc7ae370, item 1) Hoist + block-wrap a
                                // subscripted by-ref arg place (see the free-func
                                // by-ref path) so its index temp runs before `&mut`.
                                let mut aprelude = Vec::new();
                                let place = self.emit_place_hoisted(a, &mut aprelude)?;
                                let borrow = self.byref_borrow(a, &place);
                                mparts.push(Self::hoist_wrap(&aprelude, borrow));
                            } else {
                                mparts.push(self.emit_consuming(a)?);
                            }
                        }
                        return Ok(Some(format!("{}.{}({})", obj_s, method, mparts.join(", "))));
                    }
                    return Ok(Some(format!("{}.{}({})", obj_s, method, parts.join(", "))));
                }
        Ok(None)
    }

    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_super_method_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
    ) -> Result<Option<String>> {
                // Handle super().method(args)
                if let Expr::Attr { obj: super_call_expr, name: method_name, .. } = callee.as_ref() {
                    if let Expr::Call { callee: super_ident, args: super_args, .. } = super_call_expr.as_ref() {
                        if let Expr::Ident(n, _) = super_ident.as_ref() {
                            if n == "super" && super_args.is_empty() {
                                if let Some(_class_name) = self.current_class.clone() {
                                    // Call __super_ alias method which has parent's body
                                    let mut arg_parts = Vec::new();
                                    for a in args { arg_parts.push(self.emit_consuming(a)?); }
                                    return Ok(Some(format!("self.__super_{}({})", method_name, arg_parts.join(", "))));
                                }
                            }
                        }
                    }
                }
        Ok(None)
    }

    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_constructor_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<Option<String>> {
                // Check if this is a class constructor call.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some(class_def) = self.ctx.classes.get(name.as_str()).cloned() {
                        let has_init = class_def.methods.iter().any(|m| m.name == "__init__");

                        // Use ::new() constructor whenever __init__ is defined —
                        // including the zero-arg case so that __init__ side-effects
                        // (field assignments, etc.) always run.
                        if has_init {
                            // (EPIC-5 C2-3) The `new()` signature lowers each
                            // `__init__` param via `rust_ty`, so a base-typed param
                            // is `B__`. A raw-struct / subclass argument into such a
                            // slot must be WRAPPED in the right variant (the same
                            // wrap-or-passthrough used at return / assign / free-fn
                            // sites) — otherwise the bare `Dog::new(..)` mismatches
                            // the `Animal__` param (E0308). Non-polymorphic params
                            // keep the clone-on-use emission.
                            let mut init_params: Vec<(String, Ty)> = class_def.methods.iter()
                                .find(|m| m.name == "__init__")
                                .map(|m| m.params.iter()
                                    .filter(|p| p.name != "self")
                                    .filter_map(|p| Ty::from_type_expr(&p.ty, p.span).ok().map(|t| (p.name.clone(), t)))
                                    .collect())
                                .unwrap_or_default();
                            // Generics v2 (generic class with a `Callable[.., V]`
                            // CONSTRUCTOR PARAM): the param type lowers with the bare
                            // class type-param name (`Rc<dyn Fn() -> V>`), but `V` is
                            // NOT in scope at this CALL site — a lambda arg would be
                            // cast `as Rc<dyn Fn() -> V>` and rustc raises E0425. So
                            // substitute the class's type params with the concrete
                            // instance type args inferred for THIS constructor call
                            // (`DD(lambda: 0)` infers `V = int` from the factory's
                            // return), yielding a concrete cast (`Rc<dyn Fn() -> i64>`).
                            // A non-generic class has empty `type_params`, so the map
                            // is empty and every param type is unchanged.
                            if !class_def.type_params.is_empty() {
                                if let Ty::Class(_, inst_args) =
                                    self.type_of_expr(&Expr::Call {
                                        callee: callee.clone(),
                                        args: args.to_vec(),
                                        kwargs: kwargs.to_vec(),
                                        span: callee.span(),
                                    })
                                {
                                    let subst: std::collections::HashMap<String, Ty> = class_def
                                        .type_params
                                        .iter()
                                        .cloned()
                                        .zip(inst_args.into_iter())
                                        .filter(|(_, t)| !matches!(t, Ty::Unknown))
                                        .collect();
                                    if !subst.is_empty() {
                                        for (_, t) in init_params.iter_mut() {
                                            *t = crate::typeck::substitute_class_typarams(t, &subst);
                                        }
                                    }
                                }
                            }
                            let mut call_parts = Vec::new();
                            for (i, a) in args.iter().enumerate() {
                                call_parts.push(self.emit_arg_into_slot(a, init_params.get(i).map(|(_, t)| t))?);
                            }
                            for (kw, v) in kwargs {
                                let pt = init_params.iter().find(|(n, _)| n == kw).map(|(_, t)| t);
                                call_parts.push(self.emit_arg_into_slot(v, pt)?);
                            }
                            return Ok(Some(format!("{}::new({})", name, call_parts.join(", "))));
                        }

                        // Class constructor: emit a Rust struct literal.
                        // Use inherited + own fields for positional.
                        let mut all_field_names: Vec<String> = Vec::new();
                        for base in &class_def.bases {
                            if let Some(bd) = self.ctx.classes.get(base.as_str()).cloned() {
                                for f in &bd.fields {
                                    if !all_field_names.contains(&f.name) {
                                        all_field_names.push(f.name.clone());
                                    }
                                }
                            }
                        }
                        for f in &class_def.fields {
                            if !all_field_names.contains(&f.name) {
                                all_field_names.push(f.name.clone());
                            }
                        }

                        if !args.is_empty() && kwargs.is_empty() {
                            // Positional args to a class constructor.
                            if args.len() != all_field_names.len() {
                                return Err(crate::diag::Error::Codegen(format!(
                                    "class `{}` has {} fields but {} positional arguments given",
                                    name, all_field_names.len(), args.len()
                                )));
                            }
                            let mut parts = Vec::new();
                            for (field_name, arg) in all_field_names.iter().zip(args.iter()) {
                                // (EPIC-5 C2-3) The struct field lowers to `B__` for
                                // a polymorphic-base field, so a raw-struct/subclass
                                // value wraps in its variant (same as the ctor/new
                                // path above).
                                let fty = self.class_field_type(&class_def, field_name);
                                let v = self.emit_arg_into_slot(arg, fty.as_ref())?;
                                // (EPIC-6) Escape a keyword field name in the
                                // positional struct-literal init.
                                parts.push(format!("{}: {}", escape_ident(field_name), v));
                            }
                            return Ok(Some(format!("{} {{ {} }}", name, parts.join(", "))));
                        }

                        // Keyword-args form.
                        if !kwargs.is_empty() {
                            let mut parts = Vec::new();
                            for (kw, val) in kwargs {
                                let fty = self.class_field_type(&class_def, kw);
                                let v = self.emit_arg_into_slot(val, fty.as_ref())?;
                                // (EPIC-6) Escape a keyword field name in the
                                // keyword-arg struct-literal init.
                                parts.push(format!("{}: {}", escape_ident(kw), v));
                            }
                            return Ok(Some(format!("{} {{ {} }}", name, parts.join(", "))));
                        }

                        // No args at all: emit default struct literal.
                        let mut parts = Vec::new();
                        for fname in &all_field_names {
                            let field = class_def.fields.iter().find(|f| &f.name == fname)
                                .or_else(|| {
                                    class_def.bases.iter().find_map(|b| {
                                        self.ctx.classes.get(b.as_str())
                                            .and_then(|bd| bd.fields.iter().find(|f| &f.name == fname))
                                    })
                                });
                            let default = if let Some(f) = field {
                                let ty = Ty::from_type_expr(&f.ty, f.span)?;
                                self.zeroed_default(&ty)
                            } else {
                                "Default::default()".to_string()
                            };
                            // (EPIC-6) Escape a keyword field name in the no-arg
                            // default struct-literal init.
                            parts.push(format!("{}: {}", escape_ident(fname), default));
                        }
                        return Ok(Some(format!("{} {{ {} }}", name, parts.join(", "))));
                    }
                }
        Ok(None)
    }

    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_builtin_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<Option<String>> {
                // Multi-arg print with inline format
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "print" {
                        if args.is_empty() {
                            return Ok(Some("println!(\"\")".to_string()));
                        }
                        let mut parts: Vec<String> = Vec::new();
                        for arg in args {
                            let raw = self.emit_expr(arg)?;
                            let formatted = match self.type_of_expr(arg) {
                                Ty::Float => format!("__py_fmt_float({})", raw),
                                Ty::Bool => format!("__py_fmt_bool({})", raw),
                                Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                    format!("({}).py_repr()", raw),
                                _ => raw,
                            };
                            parts.push(formatted);
                        }
                        // Use {} (Display format) for most types; {:?} breaks strings by adding quotes
                        let fmt = (0..parts.len()).map(|_| "{}").collect::<Vec<_>>().join(" ");
                        return Ok(Some(format!("println!(\"{}\" {})", fmt,
                            if parts.is_empty() { "".to_string() } else { format!(", {}", parts.join(", ")) })));
                    }
                }

                // Inline range() with 1, 2, or 3 args
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "range" {
                        if args.len() == 1 {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(0..{})", a)));
                        } else if args.len() == 2 {
                            let a = self.emit_expr(&args[0])?;
                            let b = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("({}..{})", a, b)));
                        } else if args.len() == 3 {
                            let a = self.emit_expr(&args[0])?;
                            let b = self.emit_expr(&args[1])?;
                            let step = self.emit_expr(&args[2])?;
                            return Ok(Some(format!("({}..{}).step_by({} as usize)", a, b, step)));
                        }
                    }
                }

                // Inline enumerate(iter) — emits iterator chain without collecting
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "enumerate" && args.len() == 1 {
                        let a = self.emit_expr(&args[0])?;
                        let is_range = a.contains("..");
                        let iter_chain = if is_range {
                            format!("({}).into_iter()", a)
                        } else if matches!(self.type_of_expr(&args[0]), Ty::Iterator(_)) {
                            // (LAZY-GEN V1-c) A generator source (`Gen<T>`) is
                            // itself an `Iterator` yielding owned `T` — consume
                            // it directly (no `.iter()`, `Gen` has none).
                            Self::iter_arg_source(&args[0], &a)
                        } else {
                            format!("{}.iter().cloned()", a)
                        };
                        return Ok(Some(format!("{}.enumerate().map(|(i, v)| (i as i64, v))", iter_chain)));
                    }
                }

                // Inline zip(a, b) — emits iterator chain without collecting
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "zip" && args.len() == 2 {
                        let a = self.emit_expr(&args[0])?;
                        let b = self.emit_expr(&args[1])?;
                        let is_range_a = a.contains("..");
                        let is_range_b = b.contains("..");
                        // (LAZY-GEN V1-c) Either side may independently be a
                        // generator (`Ty::Iterator`) — a mixed generator+list
                        // `zip` is valid Python, so each side is classified on
                        // its own type, not the other's shape.
                        let iter_a = if is_range_a {
                            format!("({}).into_iter()", a)
                        } else if matches!(self.type_of_expr(&args[0]), Ty::Iterator(_)) {
                            Self::iter_arg_source(&args[0], &a)
                        } else {
                            format!("{}.iter().cloned()", a)
                        };
                        let iter_b = if is_range_b {
                            format!("({}).into_iter()", b)
                        } else if matches!(self.type_of_expr(&args[1]), Ty::Iterator(_)) {
                            Self::iter_arg_source(&args[1], &b)
                        } else {
                            format!("{}.iter().cloned()", b)
                        };
                        return Ok(Some(format!("{}.zip({})", iter_a, iter_b)));
                    }
                }

                // Builtin function dispatch
                if let Expr::Ident(n, _) = callee.as_ref() {
                    match n.as_str() {
                        "len" => {
                            let a = self.emit_expr(&args[0])?;
                            // Python len() of a str is the CHARACTER count, not the
                            // UTF-8 byte count. Collections keep .len().
                            if matches!(self.type_of_expr(&args[0]), Ty::Str) {
                                return Ok(Some(format!("{}.chars().count() as i64", a)));
                            }
                            return Ok(Some(format!("{}.len() as i64", a)));
                        }
                        "str" => {
                            let a = self.emit_expr(&args[0])?;
                            match self.type_of_expr(&args[0]) {
                                // Match print/f-string formatting: a whole float is
                                // "7.0" (Rust's `{}` would drop it to "7"), a bool is
                                // "True"/"False" (not Rust's "true"/"false").
                                Ty::Float => return Ok(Some(format!("__py_fmt_float({})", a))),
                                Ty::Bool => return Ok(Some(format!("__py_fmt_bool({})", a))),
                                Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                    return Ok(Some(format!("({}).py_repr()", a))),
                                _ => return Ok(Some(format!("format!(\"{{}}\" , {})", a))),
                            }
                        }
                        "open" => {
                            let path = self.emit_expr(&args[0])?;
                            let mode = if args.len() >= 2 {
                                self.emit_expr(&args[1])?
                            } else {
                                "\"r\".to_string()".to_string()
                            };
                            return Ok(Some(format!("__py_open(&{}, &{})", path, mode)));
                        }
                        "int" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    // Use helper so a bad string panics with "ValueError\0..."
                                    // which the try/except dispatcher can match on ValueError.
                                    return Ok(Some(format!("(__py_int_from_str(&{}))", a)));
                                }
                                _ => return Ok(Some(format!("({} as i64)", a))),
                            }
                        }
                        "float" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    // Use helper so a bad string panics with "ValueError\0..."
                                    // which the try/except dispatcher can match on ValueError.
                                    return Ok(Some(format!("(__py_float_from_str(&{}))", a)));
                                }
                                _ => return Ok(Some(format!("({} as f64)", a))),
                            }
                        }
                        "bool" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(({}) != 0)", a)));
                        }
                        "abs" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({}).abs()", a)));
                        }
                        "min" => {
                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // min with key parameter
                                let a = self.emit_expr(&args[0])?;
                                // Check if key_expr is a Lambda to handle it specially
                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), Ty::Unknown);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                return Ok(Some(format!(
                                    "{{ let __list = {}; __list.iter().min_by_key(|__x| {}).map(|__x| __x.clone()).unwrap_or_default() }}",
                                    a, key_code
                                )));
                            } else if args.len() == 1 {
                                let a = self.emit_expr(&args[0])?;
                                let arg_ty = self.type_of_expr(&args[0]);
                                let elem_ty = match &arg_ty {
                                    Ty::List(inner) | Ty::Iterator(inner) => (**inner).clone(),
                                    _ => Ty::Int,
                                };
                                // (LAZY-GEN V1-c) A generator source (`Gen<T>`)
                                // yields OWNED elements directly — no `.iter()`
                                // (`Gen` has none) and no `&`/deref on `__x`.
                                if matches!(arg_ty, Ty::Iterator(_)) {
                                    let src = Self::iter_arg_source(&args[0], &a);
                                    return Ok(Some(match elem_ty {
                                        Ty::Float => format!("{{ let mut __min = f64::INFINITY; for __x in {} {{ if __x < __min {{ __min = __x; }} }} __min }}", src),
                                        _ => format!("{}.min().unwrap_or(0)", src),
                                    }));
                                }
                                return Ok(Some(match elem_ty {
                                    Ty::Float => format!("{{ let mut __min = f64::INFINITY; for __x in {}.iter() {{ if __x < &__min {{ __min = *__x; }} }} __min }}", a),
                                    _ => format!("{}.iter().copied().min().unwrap_or(0)", a),
                                }));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let b = self.emit_expr(&args[1])?;
                                return Ok(Some(format!("::std::cmp::min({}, {})", a, b)));
                            }
                        }
                        "max" => {
                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // max with key parameter
                                let a = self.emit_expr(&args[0])?;
                                // Check if key_expr is a Lambda to handle it specially
                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), Ty::Unknown);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                return Ok(Some(format!(
                                    "{{ let __list = {}; __list.iter().max_by_key(|__x| {}).map(|__x| __x.clone()).unwrap_or_default() }}",
                                    a, key_code
                                )));
                            } else if args.len() == 1 {
                                let a = self.emit_expr(&args[0])?;
                                let arg_ty = self.type_of_expr(&args[0]);
                                let elem_ty = match &arg_ty {
                                    Ty::List(inner) | Ty::Iterator(inner) => (**inner).clone(),
                                    _ => Ty::Int,
                                };
                                // (LAZY-GEN V1-c) See the `min` arm above: a
                                // generator source yields OWNED elements — no
                                // `.iter()`/deref.
                                if matches!(arg_ty, Ty::Iterator(_)) {
                                    let src = Self::iter_arg_source(&args[0], &a);
                                    return Ok(Some(match elem_ty {
                                        Ty::Float => format!("{{ let mut __max = f64::NEG_INFINITY; for __x in {} {{ if __x > __max {{ __max = __x; }} }} __max }}", src),
                                        _ => format!("{}.max().unwrap_or(0)", src),
                                    }));
                                }
                                return Ok(Some(match elem_ty {
                                    Ty::Float => format!("{{ let mut __max = f64::NEG_INFINITY; for __x in {}.iter() {{ if __x > &__max {{ __max = *__x; }} }} __max }}", a),
                                    _ => format!("{}.iter().copied().max().unwrap_or(0)", a),
                                }));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let b = self.emit_expr(&args[1])?;
                                return Ok(Some(format!("::std::cmp::max({}, {})", a, b)));
                            }
                        }
                        "sorted" => {
                            let a = self.emit_expr(&args[0])?;
                            let list_ty = self.type_of_expr(&args[0]);
                            // (LAZY-GEN V1-c) A generator source can't be
                            // `.clone()`d (`Gen<T>` isn't `Clone`) — materialize
                            // it into an owned `Vec<T>` via `.collect()` first
                            // (a VARIABLE source is consumed `&mut`, Python-exact
                            // "exhausted but still bound"; a fresh call is
                            // consumed by value — see `iter_arg_source`).
                            // `sorted(...)` always returns a real `list[T]`,
                            // matching Python (which also materializes a
                            // generator when sorting it).
                            let list_src = if matches!(list_ty, Ty::Iterator(_)) {
                                format!("{}.collect::<Vec<_>>()", Self::iter_arg_source(&args[0], &a))
                            } else {
                                format!("{}.clone()", a)
                            };

                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // sorted with key parameter
                                // Determine the return type of the key expression
                                let key_ret_ty = if let Expr::Lambda { params: _, body, .. } = key_expr {
                                    // For lambdas, infer from the body expression
                                    // We need to temporarily register the parameter to type-check the body
                                    // But since type_of_expr is &self, we can't do that easily
                                    // So we'll just check common patterns
                                    if let Expr::Attr { name, .. } = body.as_ref() {
                                        // Lambda body is field access - check the field type
                                        // (LAZY-GEN V1-c) A generator source's element
                                        // class fields resolve the same way a list's do.
                                        if let Ty::List(ref elem_ty) | Ty::Iterator(ref elem_ty) = list_ty {
                                            if let Ty::Class(cls, _) = elem_ty.as_ref() {
                                                if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                                                    if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                                                        Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Unknown)
                                                    } else {
                                                        Ty::Unknown
                                                    }
                                                } else {
                                                    Ty::Unknown
                                                }
                                            } else {
                                                Ty::Unknown
                                            }
                                        } else {
                                            Ty::Unknown
                                        }
                                    } else if let Expr::Call { callee, .. } = body.as_ref() {
                                        // Lambda body is a method call - check method return type
                                        if let Expr::Attr { name, .. } = callee.as_ref() {
                                            // (LAZY-GEN V1-c) Same as above: a
                                            // generator source's element methods
                                            // resolve the same way a list's do.
                                            if let Ty::List(ref elem_ty) | Ty::Iterator(ref elem_ty) = list_ty {
                                                if let Ty::Class(cls, _) = elem_ty.as_ref() {
                                                    if let Some(method_sig) = self.ctx.get_method(cls.as_str(), name) {
                                                        method_sig.ret.clone()
                                                    } else {
                                                        Ty::Unknown
                                                    }
                                                } else {
                                                    Ty::Unknown
                                                }
                                            } else {
                                                Ty::Unknown
                                            }
                                        } else {
                                            Ty::Unknown
                                        }
                                    } else {
                                        Ty::Unknown
                                    }
                                } else {
                                    Ty::Unknown
                                };

                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), Ty::Unknown);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };

                                // (CARD 0bab32ed) `reverse=` was being silently
                                // dropped whenever `key=` was also present. CPython's
                                // `reverse=True` is a REVERSED STABLE SORT: elements
                                // with equal keys keep their ORIGINAL relative order.
                                // That is NOT the same as sorting ascending and then
                                // `.reverse()`-ing the whole vec, which flips the
                                // relative order within an equal-key run. Verified
                                // against `python3`:
                                //   sorted([(1,'a'),(2,'b'),(1,'c')], key=lambda t: t[0], reverse=True)
                                //   -> [(2,'b'),(1,'a'),(1,'c')]   (a before c, unchanged)
                                // A reversed COMPARATOR fed into Rust's stable
                                // `sort_by` reproduces this exactly: when two keys
                                // are equal, `.reverse()`'d Ordering::Equal is still
                                // Equal, so the stable sort leaves their relative
                                // input order untouched; only actually-unequal pairs
                                // flip. Handle a runtime (non-literal) `reverse=`
                                // expression too, not just `True`/`False` literals.
                                if let Some((_, rev_expr)) = kwargs.iter().find(|(n, _)| n == "reverse") {
                                    let rev_s = self.emit_expr(rev_expr)?;
                                    return Ok(Some(match key_ret_ty {
                                        Ty::Float => {
                                            format!(
                                                "{{ let mut __sorted = {}; let __rev = {}; __sorted.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; let __ord = ka.partial_cmp(&kb).unwrap_or(::std::cmp::Ordering::Equal); if __rev {{ __ord.reverse() }} else {{ __ord }} }}); __sorted }}",
                                                list_src, rev_s, key_code, key_code
                                            )
                                        }
                                        _ => {
                                            format!(
                                                "{{ let mut __sorted = {}; let __rev = {}; __sorted.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; let __ord = ka.cmp(&kb); if __rev {{ __ord.reverse() }} else {{ __ord }} }}); __sorted }}",
                                                list_src, rev_s, key_code, key_code
                                            )
                                        }
                                    }));
                                }

                                // Use appropriate sorting method based on key return type
                                return Ok(Some(match key_ret_ty {
                                    Ty::Float => {
                                        format!(
                                            "{{ let mut __sorted = {}; __sorted.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; ka.partial_cmp(&kb).unwrap_or(::std::cmp::Ordering::Equal) }}); __sorted }}",
                                            list_src, key_code, key_code
                                        )
                                    }
                                    _ => {
                                        format!(
                                            "{{ let mut __sorted = {}; __sorted.sort_by_key(|__x| {}); __sorted }}",
                                            list_src, key_code
                                        )
                                    }
                                }));
                            } else {
                                // Check if this is a float list to handle Ord constraint
                                // (a float GENERATOR needs the same partial_cmp
                                // treatment — f64 isn't Ord — once collected).
                                let is_float_list = matches!(&list_ty,
                                    Ty::List(inner) | Ty::Iterator(inner) if inner.as_ref() == &Ty::Float);
                                let sort_code = if is_float_list {
                                    ".sort_by(|a, b| a.partial_cmp(b).unwrap_or(::std::cmp::Ordering::Equal))".to_string()
                                } else {
                                    ".sort()".to_string()
                                };

                                // `sorted` operates on a Vec. A list arg is cloned
                                // directly; a set is materialized from its elements;
                                // a dict from its KEYS (Python semantics — both
                                // HashMap/HashSet lack `.sort()`); a generator via
                                // `list_src` (`.collect()`, computed above).
                                let base = match &list_ty {
                                    Ty::Set(_) => format!("{}.iter().cloned().collect::<Vec<_>>()", a),
                                    Ty::Dict(_, _) => format!("{}.keys().cloned().collect::<Vec<_>>()", a),
                                    _ => list_src.clone(),
                                };

                                if let Some((_, rev_expr)) = kwargs.iter().find(|(n, _)| n == "reverse") {
                                    // sorted with reverse parameter
                                    let rev_s = self.emit_expr(rev_expr)?;
                                    return Ok(Some(format!(
                                        "{{ let mut __sorted = {}; __sorted{}; if {} {{ __sorted.reverse(); }} __sorted }}",
                                        base, sort_code, rev_s
                                    )));
                                } else {
                                    // Default sorted
                                    return Ok(Some(format!("{{ let mut __sorted = {}; __sorted{}; __sorted }}", base, sort_code)));
                                }
                            }
                        }
                        "sum" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_ty = self.type_of_expr(&args[0]);
                            // Determine the sum type based on the iterable's element type
                            let sum_type = match &arg_ty {
                                Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => match inner.as_ref() {
                                    Ty::Float => "f64",
                                    _ => "i64",
                                },
                                _ => "i64",
                            };
                            // (LAZY-GEN V1-c) A generator source (`Gen<T>`) is
                            // itself an `Iterator` yielding owned `T` — consume
                            // it directly (no `.iter()`, `Gen` has none).
                            if matches!(arg_ty, Ty::Iterator(_)) {
                                let src = Self::iter_arg_source(&args[0], &a);
                                return Ok(Some(format!("{}.sum::<{}>()", src, sum_type)));
                            }
                            return Ok(Some(format!("{}.iter().sum::<{}>()", a, sum_type)));
                        }
                        "input" => {
                            if args.is_empty() {
                                return Ok(Some("{ let mut __s = String::new(); ::std::io::stdin().read_line(&mut __s).unwrap(); __s.trim_end().to_string() }".to_string()));
                            } else {
                                let p = self.emit_expr(&args[0])?;
                                return Ok(Some(format!("{{ print!(\"{{}}\" , {}); ::std::io::stdout().flush().ok(); let mut __s = String::new(); ::std::io::stdin().read_line(&mut __s).unwrap(); __s.trim_end().to_string() }}", p)));
                            }
                        }
                        "any" => {
                            let a = self.emit_expr(&args[0])?;
                            // (LAZY-GEN V1-c) A generator source yields owned
                            // `bool` directly (no `.iter()`/deref); short-circuit
                            // laziness is Rust `Iterator::any`'s native behavior,
                            // matching Python.
                            if matches!(self.type_of_expr(&args[0]), Ty::Iterator(_)) {
                                let src = Self::iter_arg_source(&args[0], &a);
                                return Ok(Some(format!("{}.any(|x| x)", src)));
                            }
                            return Ok(Some(format!("{}.iter().any(|x| *x)", a)));
                        }
                        "all" => {
                            let a = self.emit_expr(&args[0])?;
                            if matches!(self.type_of_expr(&args[0]), Ty::Iterator(_)) {
                                let src = Self::iter_arg_source(&args[0], &a);
                                return Ok(Some(format!("{}.all(|x| x)", src)));
                            }
                            return Ok(Some(format!("{}.iter().all(|x| *x)", a)));
                        }
                        "round" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({} as f64).round() as i64", a)));
                        }
                        "pow" => {
                            let base = self.emit_expr(&args[0])?;
                            let exp = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("({} as f64).powi({} as i32) as i64", base, exp)));
                        }
                        "chr" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(char::from_u32({} as u32).unwrap()).to_string()", a)));
                        }
                        "ord" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({}.chars().next().unwrap() as i64)", a)));
                        }
                        "reversed" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("{{ let mut __r = {}.clone(); __r.reverse(); __r }}", a)));
                        }
                        "map" => {
                            let f = self.emit_expr(&args[0])?;
                            let it = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("{}.iter().cloned().map({}).collect::<Vec<_>>()", it, f)));
                        }
                        "filter" => {
                            let f = self.emit_expr(&args[0])?;
                            let it = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("{}.iter().cloned().filter(|__x| ({})((__x).clone())).collect::<Vec<_>>()", it, f)));
                        }
                        "isinstance" => {
                            if args.len() != 2 {
                                return Err(crate::diag::Error::Codegen("isinstance requires exactly 2 arguments".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            // Check if args[1] is a builtin type identifier
                            if let Expr::Ident(type_name, _) = &args[1] {
                                let matches = match type_name.as_str() {
                                    "int" => matches!(&obj_type, Ty::Int),
                                    "str" => matches!(&obj_type, Ty::Str),
                                    "float" => matches!(&obj_type, Ty::Float),
                                    "bool" => matches!(&obj_type, Ty::Bool),
                                    "list" => matches!(&obj_type, Ty::List(_)),
                                    "dict" => matches!(&obj_type, Ty::Dict(_, _)),
                                    "set" => matches!(&obj_type, Ty::Set(_)),
                                    _ => {
                                        // For custom classes, emit runtime check
                                        let _obj = self.emit_expr(&args[0])?;
                                        return Ok(Some(format!("true"))); // Placeholder for custom class check
                                    }
                                };
                                return Ok(Some(if matches { "true" } else { "false" }.to_string()));
                            } else {
                                // Dynamic type check (not a literal type name)
                                return Ok(Some("true".to_string())); // Conservative: assume true for dynamic checks
                            }
                        }
                        "type" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("type requires exactly 1 argument".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            let type_name = match obj_type {
                                Ty::Int => "<class 'int'>",
                                Ty::Str => "<class 'str'>",
                                Ty::Float => "<class 'float'>",
                                Ty::Bool => "<class 'bool'>",
                                Ty::List(_) => "<class 'list'>",
                                Ty::Dict(_, _) => "<class 'dict'>",
                                Ty::Set(_) => "<class 'set'>",
                                // Both the `None` literal (NoneVal) and a void
                                // result (Unit) report Python's NoneType, matching
                                // the pre-NoneVal behavior of `type(None)`.
                                Ty::Unit | Ty::NoneVal => "<class 'NoneType'>",
                                _ => "<class 'object'>",
                            };
                            return Ok(Some(format!("String::from(\"{}\")", type_name)));
                        }
                        "hex" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#x}}\", {})", a)));
                        }
                        "oct" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#o}}\", {})", a)));
                        }
                        "bin" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#b}}\", {})", a)));
                        }
                        "callable" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("callable requires exactly 1 argument".into()));
                            }
                            // Check if the argument is a function name
                            if let Expr::Ident(name, _) = &args[0] {
                                let is_callable = self.ctx.funcs.contains_key(name.as_str()) ||
                                                 self.ctx.classes.contains_key(name.as_str());
                                return Ok(Some(if is_callable { "true" } else { "false" }.to_string()));
                            } else {
                                // For non-identifier expressions, conservatively return false
                                return Ok(Some("false".to_string()));
                            }
                        }
                        "repr" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("repr requires exactly 1 argument".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            let a = self.emit_expr(&args[0])?;
                            let repr_expr = match obj_type {
                                Ty::Str => format!("format!(\"'{{}}'\", {})", a),
                                Ty::Bool => format!("format!(\"{{}}\" , if {} {{ \"True\" }} else {{ \"False\" }})", a),
                                _ => format!("format!(\"{{}}\" , {})", a),
                            };
                            return Ok(Some(repr_expr));
                        }
                        "ascii" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("ascii requires exactly 1 argument".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            let a = self.emit_expr(&args[0])?;
                            let ascii_expr = match obj_type {
                                Ty::Str => {
                                    format!(
                                        "format!(\"'{{}}'\", {}.escape_default())",
                                        a
                                    )
                                }
                                Ty::Bool => {
                                    format!("format!(\"{{}}\" , if {} {{ \"True\" }} else {{ \"False\" }})", a)
                                }
                                _ => format!("format!(\"{{}}\" , {})", a),
                            };
                            return Ok(Some(ascii_expr));
                        }
                        "list" => {
                            if args.is_empty() {
                                return Ok(Some("Vec::<i64>::new()".to_string()));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let arg_type = self.type_of_expr(&args[0]);
                                // If the argument is already a list, just return it. Otherwise collect the iterator.
                                match arg_type {
                                    Ty::List(_) => return Ok(Some(a)),
                                    // (LAZY-GEN V1-c, review finding) A generator
                                    // arg is TYPE-DRIVEN, not sniffed from the
                                    // emitted call text — a generator function
                                    // named e.g. `unsorted_gen` would otherwise
                                    // mis-fire the "looks like a Vec already"
                                    // heuristic below (its emitted call text
                                    // contains the substring "sort") and skip the
                                    // needed `.collect()`. This explicit arm runs
                                    // FIRST, so the happy path is deliberate, not
                                    // an accident of the fallback.
                                    Ty::Iterator(_) => {
                                        let src = Self::iter_arg_source(&args[0], &a);
                                        return Ok(Some(format!("{}.collect::<Vec<_>>()", src)));
                                    }
                                    // A set/dict is a concrete container, not an
                                    // iterator: take an owned Vec of its elements
                                    // (dict -> its KEYS, Python semantics).
                                    Ty::Set(_) => {
                                        return Ok(Some(format!("{}.iter().cloned().collect::<Vec<_>>()", a)));
                                    }
                                    Ty::Dict(_, _) => {
                                        return Ok(Some(format!("{}.keys().cloned().collect::<Vec<_>>()", a)));
                                    }
                                    _ => {
                                        // Check if the expression looks like it returns a Vec (contains reverse, sort, etc.)
                                        if a.contains("reverse") || a.contains("sort") || a.contains("clone()") {
                                            return Ok(Some(a));
                                        }
                                        return Ok(Some(format!("{}.collect::<Vec<_>>()", a)));
                                    }
                                }
                            }
                        }
                        "dict" => {
                            if args.is_empty() && kwargs.is_empty() {
                                return Ok(Some("std::collections::HashMap::new()".to_string()));
                            } else {
                                return Err(crate::diag::Error::Codegen("dict() constructor with arguments not yet supported".into()));
                            }
                        }
                        "tuple" => {
                            if args.is_empty() {
                                return Ok(Some("()".to_string()));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                return Ok(Some(format!("({},)", a)));
                            }
                        }
                        "getattr" => {
                            if args.len() < 2 || args.len() > 3 {
                                return Err(crate::diag::Error::Codegen("getattr requires 2 or 3 arguments".into()));
                            }
                            let _obj = self.emit_expr(&args[0])?;
                            let attr_name = self.emit_expr(&args[1])?;

                            // For now, just access the field directly (works for simple cases)
                            // This assumes the object is a struct with the matching field name
                            return Ok(Some(format!("{{ let __attr_name = {}; format!(\"{{:?}}\", __attr_name) }}", attr_name)));
                        }
                        "setattr" => {
                            if args.len() != 3 {
                                return Err(crate::diag::Error::Codegen("setattr requires exactly 3 arguments".into()));
                            }
                            // Note: In Python, setattr modifies the object. In Rust, we can't modify through a reference.
                            // For now, just return None
                            return Ok(Some("()".to_string()));
                        }
                        "hasattr" => {
                            if args.len() != 2 {
                                return Err(crate::diag::Error::Codegen("hasattr requires exactly 2 arguments".into()));
                            }
                            // For now, just return true (conservative assumption)
                            return Ok(Some("true".to_string()));
                        }
                        "set" => {
                            if args.is_empty() {
                                return Ok(Some("::std::collections::HashSet::new()".to_string()));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let arg_type = self.type_of_expr(&args[0]);
                                // If the argument is already a set, just return it. Otherwise convert to set.
                                match arg_type {
                                    Ty::Set(_) => return Ok(Some(a)),
                                    Ty::List(_) | Ty::Unknown => {
                                        // Check if it looks like a vec literal or variable
                                        if a.starts_with("vec!") {
                                            return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                        } else {
                                            return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                        }
                                    }
                                    _ => {
                                        // For other iterables, try to convert
                                        return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
        Ok(None)
    }
    pub(crate) fn emit_expr(&mut self, e: &Expr) -> Result<String> {
        Ok(match e {
            // A numeric literal is a primary expression, so bare `0i64` / `1.5f64`
            // is precedence-safe in every position (receiver, exponent, as-cast,
            // operand). The lexer only produces non-negative literals (a leading
            // `-` is a separate `UnOp::Neg` node that adds its own parens), BUT
            // `try_fold_const` can fold e.g. `2 - 5` into `Expr::Int(-3)`, so the
            // guard below is load-bearing: a negative literal must parenthesize
            // (`(-3i64)`) or it would bind as `-(3i64.method())` in receiver
            // position. Non-negative literals emit bare (was unconditionally
            // `(..)`).
            Expr::Int(n, _) => {
                let lit = format!("{}i64", n);
                if lit.starts_with('-') { format!("({})", lit) } else { lit }
            }
            Expr::Float(f, _) => {
                let lit = format!("{}f64", f);
                if lit.starts_with('-') { format!("({})", lit) } else { lit }
            }
            Expr::Bool(b, _) => b.to_string(),
            Expr::None_(_) => "None".to_string(),
            Expr::Str(s, _) => format!("String::from({:?})", s),
            Expr::FStr(parts, _) => {
                let mut fmt_str = String::new();
                let mut args = Vec::new();
                for part in parts {
                    match part {
                        crate::ast::FStrPart::Lit(s) => {
                            // Escape { and } in the format string
                            fmt_str.push_str(&s.replace('{', "{{").replace('}', "}}"));
                        }
                        crate::ast::FStrPart::Interp(expr, spec) => {
                            match spec {
                                None => {
                                    // No spec: match print()'s Python-style Display
                                    // so bare floats/bools render as `1.0` / `True`.
                                    fmt_str.push_str("{}");
                                    let raw = self.emit_expr(expr)?;
                                    let arg = match self.type_of_expr(expr) {
                                        Ty::Float => format!("__py_fmt_float({})", raw),
                                        Ty::Bool => format!("__py_fmt_bool({})", raw),
                                        Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                            format!("({}).py_repr()", raw),
                                        _ => raw,
                                    };
                                    args.push(arg);
                                }
                                Some(s) => {
                                    // Explicit spec: emit a Rust format spec and pass the
                                    // raw value (the spec drives formatting, e.g. {:.2}).
                                    let clean = s.trim_end_matches(|c: char| "fdsge%".contains(c));
                                    fmt_str.push_str(&format!("{{:{}}}", clean));
                                    args.push(self.emit_expr(expr)?);
                                }
                            }
                        }
                    }
                }
                if args.is_empty() {
                    format!("String::from(\"{}\")", fmt_str)
                } else {
                    format!("format!(\"{}\", {})", fmt_str, args.join(", "))
                }
            }
            Expr::List(elems, _) => {
                // (EPIC-5 C2-2b-i, Step 3) A list literal whose elements' common
                // type is a polymorphic base is `Vec<B__>`: each raw-struct/ctor
                // element is wrapped into its enum variant (`[Dog(), Cat()]` ->
                // `vec![Animal__::Dog(..), Animal__::Cat(..)]`). A list of already-
                // `B__` places passes through element-wise. (list+list `+` CONCAT
                // element wrapping stays a documented C2-3 gap — not handled here.)
                if let Some(base) = self.list_poly_base(elems) {
                    let base_ty = Ty::Class(base, vec![]);
                    let mut parts = Vec::with_capacity(elems.len());
                    for e in elems { parts.push(self.emit_into_base_slot(e, &base_ty)?); }
                    return Ok(format!("vec![{}]", parts.join(", ")));
                }
                // When the literal's unified element type is Float but some
                // elements are int literals (`[1, 2.0]`), cast the int elements
                // to f64 so the vec is a homogeneous `Vec<f64>` (card 5c2f31d8).
                let widen = matches!(self.list_elem_ty(elems), Ty::Float);
                let mut parts = Vec::new();
                for e in elems { parts.push(self.emit_collection_elem(e, widen)?); }
                format!("vec![{}]", parts.join(", "))
            }
            Expr::Tuple(elems, _) => {
                let parts: Result<Vec<_>> = elems.iter().map(|e| self.emit_consuming(e)).collect();
                let parts = parts?;
                match parts.len() {
                    0 => "()".to_string(),
                    1 => format!("({},)", parts[0]),
                    _ => format!("({})", parts.join(", ")),
                }
            }
            Expr::ListComp { elt, targets, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                // (CARD fd65dc99) `zip(...)`/`enumerate(...)` are inlined by
                // `emit_expr` (above) straight into a REAL lazy Rust iterator
                // adapter chain (`.zip(..)`, `.enumerate().map(..)`) — not a
                // `.iter().cloned()`-able `Vec` — so the `.iter().cloned()`
                // fallback below doesn't compile against it (`Zip`/`Map` have no
                // `.iter()`). Detect that emitted shape the same way `Stmt::For`
                // already does (string-sniffing the generated code, since
                // codegen's own `type_of_expr` oracle types the call as a
                // `Ty::List`, not `Ty::Iterator`) and consume it directly. Must
                // be checked before `is_range`: a range nested inside
                // (`zip(range(..), ys)`) would otherwise match `is_range` on the
                // OUTER string and get double-wrapped in `.into_iter()`.
                let is_lazy_adapter = iter_s.contains(".enumerate()") || iter_s.contains(".zip(");
                let is_range = iter_s.contains("..");
                let chain = if is_lazy_adapter {
                    iter_s.clone()
                } else if is_range {
                    format!("({}).into_iter()", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    // Iterating a str yields 1-character strings (Python semantics)
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Iterator(_)) {
                    // (LAZY-GEN V1-b) A generator source (`Gen<T>`) is itself an
                    // `Iterator` yielding owned `T`; consume it DIRECTLY — no
                    // `.iter().cloned()` (`Gen` has no `.iter()`), no double clone.
                    // The map/filter_map adapters compose straight onto the `Gen`.
                    // (review fix) A generator VARIABLE is borrowed `&mut`, not
                    // moved — the binding stays live and advances in place
                    // (Python: a comprehension drains the generator; reuse then
                    // yields nothing instead of E0382).
                    if matches!(iter.as_ref(), Expr::Ident(..)) {
                        format!("(&mut {})", iter_s)
                    } else {
                        iter_s.clone()
                    }
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                // Bind the loop target(s) to the iterable's element type for the
                // closure body, so a method call on the loop variable inside it
                // (`it.get()`) resolves to the element's CLASS method (BUG: an
                // unbound loop var fell through to a dict builtin and panicked).
                let saved = self.bind_comp_targets(targets, iter);
                let elt_s = self.emit_comp_value(elt)?;
                // (EPIC-6) Escape each comprehension target in the closure pattern;
                // the elt/cond bodies reference it via emit_expr Ident (same escape).
                // A single target is a bare name; multiple targets (tuple-unpacking,
                // e.g. `[v for k, v in d.items()]`) form a tuple pattern `(k, v)`
                // (mirrors the `Stmt::For` lowering).
                let target = comp_target_pat(targets);
                let result = if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some({}) }} else {{ None }} ).collect::<Vec<_>>()",
                        chain, target, cond_s, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<Vec<_>>()", chain, target, elt_s)
                };
                self.restore_comp_targets(saved);
                result
            }
            Expr::SetComp { elt, targets, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                // (CARD fd65dc99) See the ListComp arm above: a zip/enumerate
                // source is already a real lazy Rust iterator chain, not a
                // `.iter().cloned()`-able Vec.
                let is_lazy_adapter = iter_s.contains(".enumerate()") || iter_s.contains(".zip(");
                let is_range = iter_s.contains("..");
                let chain = if is_lazy_adapter {
                    iter_s.clone()
                } else if is_range {
                    format!("({}).into_iter()", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Iterator(_)) {
                    // (LAZY-GEN V1-b) A generator source is itself an `Iterator`
                    // (`Gen<T>`) yielding owned `T`; consume it DIRECTLY — no
                    // `.iter().cloned()` (`Gen` has no `.iter()`), no double clone.
                    // (review fix) A VARIABLE source borrows `&mut` (see ListComp).
                    if matches!(iter.as_ref(), Expr::Ident(..)) {
                        format!("(&mut {})", iter_s)
                    } else {
                        iter_s.clone()
                    }
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                let saved = self.bind_comp_targets(targets, iter);
                let elt_s = self.emit_comp_value(elt)?;
                // (EPIC-6) Escape the comprehension target(s) (see ListComp above).
                let target = comp_target_pat(targets);
                let result = if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some({}) }} else {{ None }} ).collect::<::std::collections::HashSet<_>>()",
                        chain, target, cond_s, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<::std::collections::HashSet<_>>()", chain, target, elt_s)
                };
                self.restore_comp_targets(saved);
                result
            }
            Expr::DictComp { key, val, targets, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                // (CARD fd65dc99) See the ListComp arm above: a zip/enumerate
                // source is already a real lazy Rust iterator chain, not a
                // `.iter().cloned()`-able Vec.
                let is_lazy_adapter = iter_s.contains(".enumerate()") || iter_s.contains(".zip(");
                let is_range = iter_s.contains("..");
                let chain = if is_lazy_adapter {
                    iter_s.clone()
                } else if is_range {
                    format!("({}).into_iter()", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Iterator(_)) {
                    // (LAZY-GEN V1-b) A generator source is itself an `Iterator`
                    // (`Gen<T>`) yielding owned `T`; consume it DIRECTLY — no
                    // `.iter().cloned()` (`Gen` has no `.iter()`), no double clone.
                    // (review fix) A VARIABLE source borrows `&mut` (see ListComp).
                    if matches!(iter.as_ref(), Expr::Ident(..)) {
                        format!("(&mut {})", iter_s)
                    } else {
                        iter_s.clone()
                    }
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                let saved = self.bind_comp_targets(targets, iter);
                let key_s = self.emit_comp_value(key)?;
                let val_s = self.emit_comp_value(val)?;
                // (EPIC-6) Escape the comprehension target(s) (see ListComp above).
                let target = comp_target_pat(targets);
                let result = if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some(({}, {})) }} else {{ None }} ).collect::<::std::collections::HashMap<_,_>>()",
                        chain, target, cond_s, key_s, val_s)
                } else {
                    format!("{}.map(|{}| ({}, {})).collect::<::std::collections::HashMap<_,_>>()", chain, target, key_s, val_s)
                };
                self.restore_comp_targets(saved);
                result
            }
            Expr::Set(elems, _) => {
                if elems.is_empty() {
                    return Ok("::std::collections::HashSet::new()".to_string());
                }
                // Mirror the list case: cast int elements to f64 when the set's
                // unified element type is Float. NOTE: a Float-element set
                // (`HashSet<f64>`) does not compile (f64 is not Eq/Hash) and is
                // unsupported in pyrst today — this widening only keeps the
                // emission consistent with the list path; it does not make a
                // numeric set literal compilable (card 5c2f31d8).
                let widen = matches!(self.list_elem_ty(elems), Ty::Float);
                let mut items = Vec::new();
                for e in elems {
                    items.push(self.emit_collection_elem(e, widen)?);
                }
                format!("vec![{}].into_iter().collect::<::std::collections::HashSet<_>>()",
                    items.join(", "))
            }
            Expr::Dict(pairs, _) => {
                if pairs.is_empty() {
                    return Ok("::std::collections::HashMap::new()".to_string());
                }
                let mut inserts = Vec::new();
                for (k, v) in pairs {
                    let ks = self.emit_consuming(k)?;
                    let vs = self.emit_consuming(v)?;
                    inserts.push(format!("({}, {})", ks, vs));
                }
                format!("vec![{}].into_iter().collect::<::std::collections::HashMap<_,_>>()",
                    inserts.join(", "))
            }
            // (EPIC-6) THE central identifier-use emission. Covers a bare
            // variable read AND a free-function call name (a user-fn call falls
            // through to `emit_expr(callee)` here), so escaping once here keeps
            // def and every use in sync. `self` is not a keyword and passes
            // through unchanged (legitimate receiver).
            Expr::Ident(n, _) => {
                // A bare reference to a MODULE CONSTANT emits its MANGLED Rust
                // name (`mangle_const`) — never the bare pyrst name — so the const
                // can't be captured as a Rust const-pattern. A local shadowing the
                // const name keeps the local's value (locals win, matching normal
                // name resolution), so the mangling only applies when `n` is NOT a
                // local. A str const additionally recovers a `String` from its
                // `&str` const.
                if let Some(m) = self.shadow_read_name(n) {
                    // (card 575bcf3a, poison2) A read of a HOISTED local that is
                    // currently divergently shadowed inside a block resolves to the
                    // mangled shadow binding (`__pyrst_shadow_..`) that holds the
                    // block-local value, not the hidden function-scope slot. Empty
                    // shadow_map (the common case) never reaches here, so shadow-free
                    // code is byte-for-byte unchanged. A shadowed name is always a
                    // local, so this correctly precedes the module-const path.
                    m
                } else if self.const_names.contains(n) && !self.locals.contains_key(n) {
                    if self.const_strs.contains(n) {
                        format!("{}.to_string()", mangle_const(n))
                    } else {
                        mangle_const(n)
                    }
                } else {
                    escape_ident(n)
                }
            }
            Expr::Call { callee, args, kwargs, .. } => {
                self.emit_call(callee, args, kwargs)?
            }
            Expr::Attr { obj, name, .. } => {
                // Qualified MODULE CONSTANT `X.CONST` for a REAL imported module:
                // when X is a tracked module and CONST is one of its module-level
                // constants, lower to the MANGLED Rust `const __pyrst_const_CONST`
                // (the const namespace is flat, mirroring qualified module CALLS;
                // the mangling prevents const-pattern capture). A str const
                // recovers a `String` from its `&str` const. This GENERALIZES the
                // former hardcoded `math.pi`/`math.e`/`math.tau` arm — `math` is
                // now a real embedded module (`lib/math.pyrs`), so its constants
                // flow through here like any other module's.
                if let Expr::Ident(modname, _) = obj.as_ref() {
                    if self
                        .ctx
                        .module_consts
                        .get(modname)
                        .is_some_and(|cs| cs.iter().any(|(c, _)| c == name))
                    {
                        return Ok(if self.const_strs.contains(name) {
                            format!("{}.to_string()", mangle_const(name))
                        } else {
                            mangle_const(name)
                        });
                    }
                }

                let o = self.emit_expr(obj)?;
                // Check if this is a @property access
                let is_property = self.is_property_access(obj, name);
                if is_property {
                    // A @property getter call: the method name (`name`) is a user
                    // method name — escaped so a keyword-named property still
                    // compiles. (Plain field reads below are escaped likewise.)
                    format!("{}.{}()", o, escape_ident(name))
                } else if !matches!(obj.as_ref(),
                                    Expr::Ident(n, _) if n == "self"
                                        || self.concrete_struct_params.contains(n))
                    && matches!(&self.type_of_expr(obj),
                                Ty::Class(b, _) if self.is_polymorphic_base(b)) {
                    // (EPIC-5 C2-2b-i) FIELD READ through a polymorphic-base var
                    // (a local/param/field whose static type is a polymorphic base).
                    // The receiver is Rust `B__` (an enum with no fields), so a
                    // direct `.{name}` won't compile. Lower to the companion enum's
                    // field-accessor `__field_{name}()` (emitted by
                    // emit_companion_enum for every base field — only base fields
                    // are reachable here; typeck already rejects a derived-only
                    // field on a base var). `self` is EXEMPT: inside a method body
                    // `self` is the concrete struct (`Account`/`Savings`), so
                    // `self.balance` is an ordinary struct-field read. A field-WRITE
                    // through a base var is a deferred honest error (AttrAssign).
                    // The companion-enum accessor is named `__field_<name>` (the
                    // `__field_` prefix makes it a non-keyword), so it is NOT
                    // escaped here — it must match the unescaped accessor emitted
                    // by emit_companion_enum.
                    format!("{}.__field_{}()", o, name)
                } else {
                    // (EPIC-6) Ordinary struct-field read: escape a keyword field
                    // name so it matches the (escaped) struct field definition.
                    format!("{}.{}", o, escape_ident(name))
                }
            }
            Expr::Index { obj, idx, .. } => {
                // type_of_expr (not just an Ident lookup) so nested/chained
                // receivers resolve — e.g. grid["row"]["x"] sees the inner Dict.
                let obj_ty = self.type_of_expr(obj);
                let o = self.emit_expr(obj)?;
                // Tuple subscript with a literal index -> Rust field access (t.N),
                // cloned so the element can be used without moving out of the tuple.
                if let Ty::Tuple(_) = obj_ty {
                    if let Expr::Int(n, _) = idx.as_ref() {
                        return Ok(format!("({}).{}.clone()", o, n));
                    }
                }
                let i = self.emit_expr(idx)?;
                match &obj_ty {
                    Ty::Dict(k, _) => {
                        // .expect() produces a Rust message without the NUL delimiter;
                        // unwrap_or_else lets us emit a matchable "KeyError\0..." payload.
                        // A GENERIC key type (a bare `Ty::TypeVar`, e.g. a class
                        // indexing its own `dict[K, V]` field) has no `Debug` bound
                        // — we never infer `K: Debug` — so the `{:?}` form would
                        // force `K: Debug` and fail to build (E0277). For such a key
                        // emit a key-less message; the `KeyError\0` payload prefix is
                        // preserved so `try/except KeyError` still matches. A concrete
                        // key keeps the value-bearing message byte-for-byte.
                        if crate::typeck::ty_contains_typevar(k) {
                            format!("({}.get(&{}).cloned().unwrap_or_else(|| panic!(\"KeyError\\0<key>\")))", o, i)
                        } else {
                            format!("({}.get(&{}).cloned().unwrap_or_else(|| panic!(\"KeyError\\0{{:?}}\", &{})))", o, i, i)
                        }
                    }
                    Ty::Str => {
                        // String indexing with negative index support.
                        // Explicit bounds check emits "IndexError\0..." so the
                        // try/except dispatcher can catch it as IndexError.
                        //
                        // The index expression is bound ONCE to `__i_idx: i64`
                        // before use. This is required for correctness when the
                        // index is itself a *block* expression (e.g. a nested list
                        // subscript `s[xs[i]]`, which lowers to `{ ... }`): inlining
                        // the raw block at each `{} < 0` / `+ {}` / `{} as usize`
                        // site produces unparenthesized `{ block } as usize`, which
                        // is a Rust parse error ("expected expression, found `as`").
                        // Binding also evaluates a side-effecting index (e.g. a call
                        // `s[f()]`) exactly once instead of three times.
                        format!(
                            "{{ let __chars: Vec<char> = {}.chars().collect(); let __i_idx: i64 = {}; let __idx = if __i_idx < 0 {{ ((__chars.len() as i64) + __i_idx) as usize }} else {{ __i_idx as usize }}; if __idx >= __chars.len() {{ panic!(\"IndexError\\0string index out of range\") }}; __chars[__idx].to_string() }}",
                            o, i
                        )
                    }
                    _ => {
                        // List indexing with negative index support.
                        // Explicit bounds check emits "IndexError\0..." so the
                        // try/except dispatcher can catch it as IndexError.
                        //
                        // FAST PATH: when the base is a borrowable place and the
                        // index cannot mutate it, borrow the container and clone
                        // only the element (O(1) vs cloning the whole Vec). The
                        // `len(self.items) - 1` form (peek) qualifies: `len()` only
                        // shared-borrows, compatible with the `&base` read borrow.
                        if self.is_borrowable_place(obj) && self.is_borrow_safe_index(idx) {
                            format!("__py_list_get(&{}, {})", o, i)
                        } else {
                            // FALLBACK: clone the base into `__list` FIRST (so a
                            // mutating/moving index still compiles), then bounds-
                            // check and clone the element. The index is bound ONCE
                            // to `__i_idx: i64` — see the Str arm above for why
                            // (nested-subscript parse error + single-evaluation of
                            // side-effecting indices).
                            format!(
                                "{{ let __list = {}.clone(); let __i_idx: i64 = {}; let __idx = if __i_idx < 0 {{ ((__list.len() as i64) + __i_idx) as usize }} else {{ __i_idx as usize }}; if __idx >= __list.len() {{ panic!(\"IndexError\\0list index out of range\") }}; __list[__idx].clone() }}",
                                o, i
                            )
                        }
                    }
                }
            }
            Expr::Slice { obj, start, stop, step, .. } => {
                let obj_ty = self.type_of_expr(obj);
                let o = self.emit_expr(obj)?;

                match obj_ty {
                    Ty::Str => {
                        // Every string slice (with or without an explicit step)
                        // routes through __py_str_slice_step, which does all the
                        // negative-index resolution, CPython PySlice_AdjustIndices
                        // clamping, and step-directional (char-based) fill. Absent
                        // start/stop pass as `None` so the helper can apply the
                        // step-sign-dependent default at runtime; an absent step is
                        // the literal 1. Borrow the base when it is a place and no
                        // present bound can mutate it (the helper needs only `&str`);
                        // otherwise snapshot-clone the base into a local and call the
                        // SAME helper, so borrow and fallback agree exactly.
                        let start_arg = start.as_ref().map(|e| self.emit_expr(e)).transpose()?
                            .map(|s| format!("Some({})", s)).unwrap_or_else(|| "None".to_string());
                        let stop_arg = stop.as_ref().map(|e| self.emit_expr(e)).transpose()?
                            .map(|s| format!("Some({})", s)).unwrap_or_else(|| "None".to_string());
                        let step_arg = step.as_ref().map(|e| self.emit_expr(e)).transpose()?
                            .unwrap_or_else(|| "1i64".to_string());
                        let subs_safe = start.as_ref().map_or(true, |e| self.is_borrow_safe_index(e))
                            && stop.as_ref().map_or(true, |e| self.is_borrow_safe_index(e))
                            && step.as_ref().map_or(true, |e| self.is_borrow_safe_index(e));
                        if self.is_borrowable_place(obj) && subs_safe {
                            return Ok(format!("__py_str_slice_step(&{}, {}, {}, {})", o, start_arg, stop_arg, step_arg));
                        }
                        return Ok(format!("{{ let __s = {}.clone(); __py_str_slice_step(&__s, {}, {}, {}) }}", o, start_arg, stop_arg, step_arg));
                    }
                    Ty::List(_) => {
                        // List slicing with step support and negative index handling
                        match (start, stop, step) {
                            (Some(s), Some(e), None) => {
                                // Simple: x[start:stop]. __py_list_slice resolves the
                                // negative bounds, clamps to [0,len], and empties when
                                // stop <= start (no usize underflow on out-of-range).
                                let start_s = self.emit_expr(s)?;
                                let stop_s = self.emit_expr(e)?;
                                // Borrow the base when it is a place and neither
                                // bound can mutate it; else snapshot-clone and call
                                // the SAME helper so both paths agree exactly.
                                if self.is_borrowable_place(obj)
                                    && self.is_borrow_safe_index(s)
                                    && self.is_borrow_safe_index(e)
                                {
                                    format!("__py_list_slice(&{}, {}, {})", o, start_s, stop_s)
                                } else {
                                    format!(
                                        "{{ let __list = {}.clone(); __py_list_slice(&__list, {}, {}) }}",
                                        o, start_s, stop_s
                                    )
                                }
                            }
                            _ => {
                                // General (stepped / one-sided) slice. All bound
                                // handling lives in __py_list_slice_step; absent
                                // start/stop pass as `None` so the helper applies the
                                // step-sign-dependent default at runtime, and an
                                // absent step is the literal 1.
                                let start_arg = start.as_ref().map(|e| self.emit_expr(e)).transpose()?
                                    .map(|s| format!("Some({})", s)).unwrap_or_else(|| "None".to_string());
                                let stop_arg = stop.as_ref().map(|e| self.emit_expr(e)).transpose()?
                                    .map(|s| format!("Some({})", s)).unwrap_or_else(|| "None".to_string());
                                let step_val = step.as_ref().map(|e| self.emit_expr(e)).transpose()?
                                    .unwrap_or_else(|| "1i64".to_string());

                                // Borrow the base when it is a place and no present
                                // bound (start/stop/step) can mutate it; else
                                // snapshot-clone and call the SAME helper.
                                let subs_safe = start.as_ref().map_or(true, |e| self.is_borrow_safe_index(e))
                                    && stop.as_ref().map_or(true, |e| self.is_borrow_safe_index(e))
                                    && step.as_ref().map_or(true, |e| self.is_borrow_safe_index(e));
                                if self.is_borrowable_place(obj) && subs_safe {
                                    format!("__py_list_slice_step(&{}, {}, {}, {})", o, start_arg, stop_arg, step_val)
                                } else {
                                    format!(
                                        "{{ let __list = {}.clone(); __py_list_slice_step(&__list, {}, {}, {}) }}",
                                        o, start_arg, stop_arg, step_val
                                    )
                                }
                            }
                        }
                    }
                    _ => return Err(crate::diag::Error::Codegen(format!("slicing not supported for type {}", obj_ty))),
                }
            }
            Expr::BinOp { op, lhs, rhs, span } => {
                // Try constant folding first
                if let Some(folded) = try_fold_const(&Expr::BinOp {
                    op: *op,
                    lhs: lhs.clone(),
                    rhs: rhs.clone(),
                    span: *span,
                }) {
                    return self.emit_expr(&folded);
                }

                // Handle sequence repetition: "abc" * 3 and [0] * 5
                if *op == BinOp::Mul {
                    let lt = self.type_of_expr(lhs);
                    let rt = self.type_of_expr(rhs);
                    if lt == Ty::Str || rt == Ty::Str {
                        let (str_e, num_e) = if lt == Ty::Str { (lhs, rhs) } else { (rhs, lhs) };
                        let s = self.emit_expr(str_e)?;
                        let n = self.emit_expr(num_e)?;
                        return Ok(format!("{}.repeat({} as usize)", s, n));
                    }
                    if matches!(&lt, Ty::List(_)) || matches!(&rt, Ty::List(_)) {
                        let (lst_e, num_e) = if matches!(&lt, Ty::List(_)) { (lhs, rhs) } else { (rhs, lhs) };
                        let v = self.emit_expr(lst_e)?;
                        let n = self.emit_expr(num_e)?;
                        return Ok(format!(
                            "{{ let mut __rep: Vec<_> = Vec::new(); for _ in 0..({} as usize) {{ __rep.extend({}.clone().into_iter()); }} __rep }}",
                            n, v
                        ));
                    }
                }

                // Handle division - always returns float in Python.
                // (Generics v2 does NOT admit `/` on a bare `T`: pyrst `/` is true
                // float division, which Rust's `Div` does not reproduce for an
                // integer `T` — so typeck rejects `T / T` and only concrete
                // operands ever reach here.)
                if *op == BinOp::Div {
                    let l = self.emit_expr(lhs)?;
                    let r = self.emit_expr(rhs)?;
                    // Convert both operands to f64 for division
                    return Ok(format!("(({} as f64) / ({} as f64))", l, r));
                }

                // Handle set operations: union, intersection, difference
                let lt = self.type_of_expr(lhs);
                let rt = self.type_of_expr(rhs);
                if matches!(&lt, Ty::Set(_)) || matches!(&rt, Ty::Set(_)) {
                    let l = self.emit_expr(lhs)?;
                    let r = self.emit_expr(rhs)?;
                    match op {
                        BinOp::BitOr => {
                            // Set union: s1 | s2
                            return Ok(format!("{{ let mut __union = {}.clone(); __union.extend({}.iter().cloned()); __union }}", l, r));
                        }
                        BinOp::BitAnd => {
                            // Set intersection: s1 & s2
                            return Ok(format!("{{ let mut __inter = std::collections::HashSet::new(); for __x in {}.iter() {{ if {}.contains(__x) {{ __inter.insert(__x.clone()); }} }} __inter }}", l, r));
                        }
                        BinOp::BitXor => {
                            // Set symmetric difference: s1 ^ s2
                            return Ok(format!("{{ let mut __diff = {}.clone(); for __x in {}.iter() {{ if __diff.contains(__x) {{ __diff.remove(__x); }} else {{ __diff.insert(__x.clone()); }} }} __diff }}", l, r));
                        }
                        BinOp::Sub => {
                            // Set difference: s1 - s2
                            return Ok(format!("{{ let mut __diff = {}.clone(); for __x in {}.iter() {{ __diff.remove(__x); }} __diff }}", l, r));
                        }
                        _ => {}
                    }
                }

                // Handle string concatenation: str + str needs special handling
                if *op == BinOp::Add {
                    let lt = self.type_of_expr(lhs);
                    let rt = self.type_of_expr(rhs);
                    if lt == Ty::Str || rt == Ty::Str {
                        let l = self.emit_expr(lhs)?;
                        let r = self.emit_expr(rhs)?;
                        // Use format! for robust string concatenation
                        return Ok(format!(r#"format!("{{}}{{}}", {}, {})"#, l, r));
                    }
                    // (EPIC-5 C2-3) `list + list` concatenation is a PRE-EXISTING
                    // gap: typeck accepts it, but the generic numeric `+` lowering
                    // below emits `vec![..] + vec![..]`, and Rust's `Vec` has no
                    // `Add` impl — so it leaked a raw rustc E0369 (a miscompile,
                    // for ANY element type, not just subtypes). Refuse honestly
                    // here rather than emit invalid Rust; the documented workaround
                    // is `.extend()` / a comprehension. (Element-wise subtype
                    // wrapping for a base-typed result is the follow-on once concat
                    // itself is implemented.) NOT an EPIC-4 path.
                    if matches!(lt, Ty::List(_)) && matches!(rt, Ty::List(_)) {
                        return Err(crate::diag::Error::Codegen(
                            "list `+` list concatenation is not yet supported — \
                             build the combined list with `.extend()` (e.g. \
                             `xs.extend(ys)`) or a comprehension instead".into(),
                        ));
                    }
                }

                // Handle `x is None` / `x is not None` → .is_none() / .is_some()
                if matches!(op, BinOp::Is | BinOp::IsNot) && matches!(rhs.as_ref(), Expr::None_(_)) {
                    let l = self.emit_expr(lhs)?;
                    return Ok(match op {
                        BinOp::Is => format!("{}.is_none()", l),
                        BinOp::IsNot => format!("{}.is_some()", l),
                        _ => unreachable!(),
                    });
                }

                // (card cc7ae370, item 3) A class-typed arithmetic dunder
                // (`+`/`-`/`*`) lowers to a `std::ops::{Add,Sub,Mul}` impl whose
                // method takes BOTH operands BY VALUE, moving them. A bare
                // `h + h2` would therefore consume `h`/`h2`, so reusing either
                // afterward is E0382 — breaking Python value semantics. Route each
                // operand through emit_consuming (the single ownership-decision
                // point): a non-Copy class PLACE (Ident / plain field chain) is
                // `.clone()`d so the original stays usable, while a fresh owned
                // rvalue (call/ctor result, literal, nested BinOp temp) is emitted
                // bare — cloning it would double-clone. Comparisons (`==`/`<`, via
                // PartialEq/PartialOrd) borrow their operands, so they are excluded
                // and stay on the generic path unchanged.
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
                    && matches!(self.type_of_expr(lhs), Ty::Class(..))
                {
                    let l = self.emit_consuming(lhs)?;
                    let r = self.emit_consuming(rhs)?;
                    let op_s = match op {
                        BinOp::Add => "+",
                        BinOp::Sub => "-",
                        BinOp::Mul => "*",
                        _ => unreachable!(),
                    };
                    return Ok(format!("({} {} {})", l, op_s, r));
                }
                let l = self.emit_expr(lhs)?;
                let r = self.emit_expr(rhs)?;

                // Get types for numeric type conversion
                let lt = self.type_of_expr(lhs);
                let rt = self.type_of_expr(rhs);

                match op {
                    BinOp::Pow => {
                        // int ** int -> integer power (matches type_of_expr inferring Int);
                        // any float operand -> float power. Use the __py_ipow helper for
                        // the integer case so a negative exponent panics with a clear
                        // message instead of silently wrapping `as u32` to a huge value.
                        if matches!(lt, Ty::Int) && matches!(rt, Ty::Int) {
                            return Ok(format!("__py_ipow(({}), ({}))", l, r));
                        }
                        return Ok(format!("(({} as f64).powf({} as f64))", l, r));
                    }
                    BinOp::In => {
                        // Use contains_key for dicts, contains for lists/sets
                        let contains_method = match rt {
                            Ty::Dict(_, _) => format!("{}.contains_key(&{})", r, l),
                            Ty::List(_) => format!("{}.iter().any(|__x| __x == &{})", r, l),
                            Ty::Set(_) => format!("{}.contains(&{})", r, l),
                            _ => format!("{}.contains(&{})", r, l),
                        };
                        return Ok(contains_method);
                    }
                    BinOp::NotIn => {
                        // Use !contains_key for dicts, !contains for lists/sets
                        let contains_method = match rt {
                            Ty::Dict(_, _) => format!("!{}.contains_key(&{})", r, l),
                            Ty::List(_) => format!("!{}.iter().any(|__x| __x == &{})", r, l),
                            Ty::Set(_) => format!("!{}.contains(&{})", r, l),
                            _ => format!("!{}.contains(&{})", r, l),
                        };
                        return Ok(contains_method);
                    }
                    BinOp::FloorDiv => {
                        // Python `//` floors toward negative infinity; Rust integer `/`
                        // truncates toward zero and Rust float `/` does not floor at all.
                        // For integer operands use __py_floordiv which also panics on /0
                        // with a catchable ZeroDivisionError payload.
                        // For float operands keep the f64 path (float //0.0 -> INF in
                        // Python is also a ZeroDivisionError but lower-priority; noted as
                        // a known gap).
                        let is_float = matches!(lt, Ty::Float) || matches!(rt, Ty::Float);
                        if is_float {
                            return Ok(format!("((({} as f64) / ({} as f64)).floor())", l, r));
                        }
                        return Ok(format!("__py_floordiv(({}), ({}))", l, r));
                    }
                    BinOp::Mod => {
                        // Python `%` returns a result with the sign of the divisor; Rust
                        // `%` returns the sign of the dividend. Use the divisor-signed
                        // helper for ints (single evaluation), rem_euclid-style for floats.
                        let is_float = matches!(lt, Ty::Float) || matches!(rt, Ty::Float);
                        if is_float {
                            return Ok(format!(
                                "{{ let __a = ({} as f64); let __b = ({} as f64); (((__a % __b) + __b) % __b) }}",
                                l, r
                            ));
                        }
                        return Ok(format!("__py_mod(({}), ({}))", l, r));
                    }
                    _ => {
                        let op_s = match op {
                            BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
                            BinOp::Div => "/",
                            BinOp::Eq => "==", BinOp::Ne => "!=",
                            BinOp::Lt => "<", BinOp::Le => "<=",
                            BinOp::Gt => ">", BinOp::Ge => ">=",
                            BinOp::And => "&&", BinOp::Or => "||",
                            BinOp::Is => "==", BinOp::IsNot => "!=",
                            BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
                            BinOp::LShift => "<<", BinOp::RShift => ">>",
                            BinOp::In | BinOp::NotIn => unreachable!(), // handled above
                            BinOp::Pow => unreachable!(), // handled above
                            BinOp::FloorDiv | BinOp::Mod => unreachable!(), // handled above
                        };

                        // Handle numeric type promotion: int op float -> convert to float
                        // Also handle cases where type inference failed (Unknown) but we know one side is float
                        let (l_final, r_final) = match (&lt, &rt) {
                            // int op float -> promote both to float
                            (Ty::Int, Ty::Float) => (format!("({} as f64)", l), format!("({})", r)),
                            // float op int -> promote both to float
                            (Ty::Float, Ty::Int) => (format!("({})", l), format!("({} as f64)", r)),
                            // Unknown op float -> try to promote Unknown as int/numeric
                            (Ty::Unknown, Ty::Float) => (format!("({} as f64)", l), format!("({})", r)),
                            // float op Unknown -> try to promote Unknown as int/numeric
                            (Ty::Float, Ty::Unknown) => (format!("({})", l), format!("({} as f64)", r)),
                            // Both same type or non-numeric: no conversion
                            _ => (l, r),
                        };

                        format!("({} {} {})", l_final, op_s, r_final)
                    }
                }
            }
            Expr::UnOp { op, expr, span } => {
                // Try constant folding first
                if let Some(folded) = try_fold_const(&Expr::UnOp {
                    op: *op,
                    expr: expr.clone(),
                    span: *span,
                }) {
                    return self.emit_expr(&folded);
                }

                // (card cc7ae370, item 3) `-x` on a class value invokes `__neg__`
                // (`std::ops::Neg::neg(self)`), which takes the operand BY VALUE
                // (move). Clone a non-Copy class PLACE via emit_consuming so the
                // original stays usable (value semantics); a fresh owned rvalue
                // (call/BinOp temp, e.g. `-(a + b)`) is emitted bare. `not`/`~`
                // are not value-consuming dunders, so they stay on emit_expr.
                let e = if matches!(op, UnOp::Neg) && matches!(self.type_of_expr(expr), Ty::Class(..)) {
                    self.emit_consuming(expr)?
                } else {
                    self.emit_expr(expr)?
                };
                match op {
                    UnOp::Neg => format!("(-{})", e),
                    UnOp::Not => format!("(!{})", e),
                    UnOp::BitNot => format!("(!({}))", e),
                }
            }
            Expr::Lambda { params, body, .. } => {
                // Emit closure params WITHOUT a type annotation and let Rust infer
                // each param's type from the use site: the call-site argument for
                // an inline-invoked lambda `(lambda x: ...)(5)`, or the iterator
                // element type for a lambda passed to map()/filter(). Hardcoding
                // `: i64` was only correct for int iterables and broke e.g.
                // `map(lambda w: len(w), words)` over a list[str].
                // (EPIC-6) Escape each lambda param; the body references it via
                // emit_expr Ident (same escape), so `|r#type| r#type + 1` stays
                // consistent.
                let param_strs: Vec<String> = params.iter()
                    .map(|(name, _ty)| escape_ident(name))
                    .collect();
                let body_s = self.emit_expr(body)?;
                format!("|{}| {}", param_strs.join(", "), body_s)
            }
            Expr::IfExp { test, body, orelse, .. } => {
                // Python's `body if test else orelse` -> Rust's if-expression.
                let t = self.emit_expr(test)?;
                let b = self.emit_expr(body)?;
                let o = self.emit_expr(orelse)?;
                format!("(if {} {{ {} }} else {{ {} }})", t, b, o)
            }
        })
    }

    /// Lower a `match`'s arms to an `if`/`else if`/`else` chain over the owned
    /// scrutinee temp `match_val` (`__match_val`). `is_first` is true at the head of
    /// the chain (emit `if`) and false for a continuation (emit `else`/`else if`).
    ///
    /// Pattern semantics:
    ///  * A `Wildcard` (`case _:`) or `Capture` (`case y:`) with NO guard ALWAYS
    ///    matches and makes the match exhaustive — it is lowered as a real `else`
    ///    (or an UNCONDITIONAL body when it heads the chain), never as `if true {}`
    ///    with no else (which let a non-unit function fall off the end -> rustc
    ///    E0317). Any arms after it are unreachable and are dropped.
    ///  * A `Capture(name)` BINDS `name` to the subject for its guard AND body. A
    ///    GUARDED capture (`case y if g:`) therefore needs `name` in scope before
    ///    the guard is tested, so it lowers to a nested block
    ///    `{ let name = mv.clone(); if g { body } else { <rest> } }` — the binding
    ///    precedes the guard, and the remaining arms continue in the `else`.
    ///  * A `Literal`/`Or` arm (optionally guarded) tests `emit_pattern_cond`
    ///    (`&& guard`) and chains the rest into the following `else if`.
    pub(crate) fn emit_match_arms(
        &mut self,
        match_val: &str,
        arms: &[crate::ast::MatchArm],
        is_first: bool,
    ) -> Result<()> {
        use crate::ast::MatchPattern;
        let Some((arm, rest)) = arms.split_first() else {
            return Ok(()); // no arms left to emit
        };

        let capture_name = match &arm.pattern {
            MatchPattern::Capture(n) => Some(n.clone()),
            _ => None,
        };
        let is_catchall = matches!(
            &arm.pattern,
            MatchPattern::Wildcard | MatchPattern::Capture(_)
        );
        let always_matches = arm.guard.is_none() && is_catchall;

        // (1) Unguarded catchall — always matches, exhaustive. Drop unreachable rest.
        if always_matches {
            if is_first {
                // No `if`/`else` wrapper: emit the body unconditionally.
                self.emit_match_arm_body(match_val, capture_name.as_deref(), &arm.body)?;
            } else {
                self.line("} else {");
                self.emit_match_arm_body(match_val, capture_name.as_deref(), &arm.body)?;
                self.line("}");
            }
            return Ok(());
        }

        // (2) Guarded CAPTURE — bind the name before the guard, then test the guard,
        // with the remaining arms continuing in the `else`. Lowered via a nested
        // block so the binding scopes over the guard and body.
        if let (Some(name), Some(guard)) = (&capture_name, &arm.guard) {
            if is_first {
                self.line("{");
            } else {
                self.line("} else {");
            }
            self.indent += 1;
            let bind = escape_ident(name);
            self.line(&format!("let mut {} = {}.clone();", bind, match_val));
            self.declared.insert(name.clone());
            let g = self.emit_expr(guard)?;
            self.line(&format!("if {} {{", g));
            self.indent += 1;
            let __arm_scope = self.scope_enter();
            for s in &arm.body {
                self.emit_stmt(s)?;
            }
            self.scope_exit(__arm_scope);
            self.indent -= 1;
            if rest.is_empty() {
                self.line("}");
            } else {
                // Continue the remaining arms inside this guard's `else`.
                self.emit_match_arms(match_val, rest, false)?;
            }
            self.indent -= 1;
            self.line("}");
            return Ok(());
        }

        // (3) Literal / Or / guarded-non-capture arm: a discriminating test.
        let cond = self.emit_pattern_cond(match_val, &arm.pattern)?;
        let guard_str = if let Some(guard) = &arm.guard {
            let g = self.emit_expr(guard)?;
            format!(" && {}", g)
        } else {
            String::new()
        };
        if is_first {
            self.line(&format!("if {}{} {{", cond, guard_str));
        } else {
            self.line(&format!("}} else if {}{} {{", cond, guard_str));
        }
        self.indent += 1;
        let __arm_scope = self.scope_enter();
        for s in &arm.body {
            self.emit_stmt(s)?;
        }
        self.scope_exit(__arm_scope);
        self.indent -= 1;
        if rest.is_empty() {
            self.line("}");
        } else {
            self.emit_match_arms(match_val, rest, false)?;
        }
        Ok(())
    }

    /// Emit a match arm body INDENTED one level, preceded by a capture binding
    /// when the arm pattern is `case <name>:`. `match_val` is the owned scrutinee
    /// temp (`__match_val`); a `Capture` binds `let <name> = __match_val.clone();`
    /// so the body sees the subject value under `<name>` (a `.clone()` keeps the
    /// scrutinee usable for sibling arms and matches pyrst's clone-on-use; it is a
    /// no-op move for `Copy` subjects and a real clone otherwise).
    pub(crate) fn emit_match_arm_body(
        &mut self,
        match_val: &str,
        capture_name: Option<&str>,
        body: &[crate::ast::Stmt],
    ) -> Result<()> {
        self.indent += 1;
        // (card 575bcf3a) Isolate the arm body's block-scope emission state; the
        // capture binding below is captured inside this window.
        let __arm_scope = self.scope_enter();
        if let Some(name) = capture_name {
            let bind = escape_ident(name);
            self.line(&format!("let mut {} = {}.clone();", bind, match_val));
            // The binding is a `let`-declared local for the rest of this arm.
            self.declared.insert(name.to_string());
        }
        for s in body {
            self.emit_stmt(s)?;
        }
        self.scope_exit(__arm_scope);
        self.indent -= 1;
        Ok(())
    }

    pub(crate) fn emit_pattern_cond(&self, var: &str, pattern: &crate::ast::MatchPattern) -> Result<String> {
        use crate::ast::MatchPattern;
        match pattern {
            MatchPattern::Wildcard => Ok("true".to_string()),
            MatchPattern::Capture(_) => Ok("true".to_string()),
            MatchPattern::Literal(Expr::Int(n, _)) => {
                Ok(format!("{} == {}i64", var, n))
            }
            MatchPattern::Literal(Expr::Bool(b, _)) => {
                Ok(format!("{} == {}", var, b))
            }
            MatchPattern::Literal(Expr::Str(s, _)) => {
                Ok(format!("{} == {:?}", var, s))
            }
            MatchPattern::Literal(Expr::None_(_)) => {
                Ok(format!("{} == None", var))
            }
            MatchPattern::Literal(_) => {
                Ok("true".to_string())
            }
            MatchPattern::Or(patterns) => {
                let conds: Result<Vec<String>> = patterns.iter()
                    .map(|p| self.emit_pattern_cond(var, p))
                    .collect();
                let conds = conds?;
                Ok(format!("({})", conds.join(" || ")))
            }
        }
    }

    pub(crate) fn line(&mut self, s: &str) {
        for _ in 0..self.indent { self.out.push_str("    "); }
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// Fold the declaration-hoisting DOUBLE-INIT artifact: a hoisted local is
    /// emitted at the top of the function as `let mut x: T = <default>;`, and its
    /// first (unconditional, top-level) assignment then re-emits as `x = <init>;`
    /// — two writes where one suffices. When the immediately-preceding emitted
    /// line is EXACTLY this variable's hoisted default declaration (same name,
    /// same type, same indent = same block, adjacent = nothing between), splice
    /// the real initializer straight into that `let` and skip the separate
    /// assignment. Fires only when the discarded initializer is the pure
    /// `default_val` (no side effect dropped) and only on true emitted adjacency,
    /// so it is exactly the card's "immediately-next statement in the same block"
    /// rule and cannot reorder or drop any effect. Returns true iff it folded.
    ///
    /// NOTE: this handles the SINGLE-hoist / adjacent case. When 2+ locals are
    /// hoisted, the sorted decl preamble separates all but the last-sorted var's
    /// decl from the buffer tail, so those do not fold and keep the double-init.
    /// The residual `unused_assignments` on that dead default is suppressed by the
    /// emitted `#![allow(..)]` header (see `emit_program`) — the order-independent
    /// completeness fix for card adc0d1c4; a name-based multi-hoist splice was
    /// prototyped but is order-sensitive (a later-sorting var folded first blocks
    /// the earlier ones), so it was not adopted.
    pub(crate) fn try_fold_hoisted_init(&mut self, target_e: &str, ty: &Ty, new_rhs: &str) -> bool {
        let def = match self.default_val(ty) {
            Some(d) => d,
            None => return false,
        };
        let rust_ty = self.rust_ty(ty);
        let indent = "    ".repeat(self.indent);
        let expected = format!("{}let mut {}: {} = {};\n", indent, target_e, rust_ty, def);
        if self.out.ends_with(&expected) {
            let keep = self.out.len() - expected.len();
            self.out.truncate(keep);
            self.line(&format!("let mut {}: {} = {};", target_e, rust_ty, new_rhs));
            true
        } else {
            false
        }
    }

    /// Maps a pyrst `Ty` to its emitted Rust type text.
    ///
    /// (EPIC-5 C2-1) This is a `Codegen` METHOD (not a free fn) specifically so
    /// the `Class` arm can consult `self.poly_map` via `is_polymorphic_base` —
    /// the method form avoids threading a `poly_map` parameter through every one
    /// of the call sites (emit_func params/returns, emit_class fields/dunder
    /// impls, emit_stmt hoists). See design §F. C2-1 is BEHAVIOR-PRESERVING: the
    /// `Class` arm still returns plain `n` for every class; the single marked
    /// hook below is what C2-2 flips to `format!("{n}__")` for a polymorphic base.
    pub(crate) fn rust_ty(&self, t: &Ty) -> String {
        match t {
            Ty::Int => "i64".into(),
            Ty::Float => "f64".into(),
            Ty::Bool => "bool".into(),
            Ty::Str => "String".into(),
            Ty::Unit => "()".into(),
            // The `None` literal's type. It never appears as a real binding
            // annotation (annotations come from `from_type_expr`, which yields
            // `Unit`/`Option`, never `NoneVal`); this arm exists for
            // exhaustiveness and mirrors `Unit` (`None` as a bare value is an
            // upstream type error).
            Ty::NoneVal => "()".into(),
            Ty::List(inner) => format!("Vec<{}>", self.rust_ty(inner)),
            // LAZY-GEN V1-b: a generator lowers to the lazy `__PyrstGen<T>`
            // coroutine (docs/design/lazy-generators.md §C.1). `__PyrstGen<T>` is a
            // concrete, nameable struct that `impl`s `Iterator<Item = T>`, so this
            // emission is uniform across return / param / field / local positions
            // (the reason it is a named struct rather than `impl Iterator`). V1-d
            // renamed it under the reserved `__Pyrst` prefix (collision-proof).
            Ty::Iterator(inner) => format!("__PyrstGen<{}>", self.rust_ty(inner)),
            Ty::Set(inner) => format!("::std::collections::HashSet<{}>", self.rust_ty(inner)),
            Ty::Dict(k, v) => format!("::std::collections::HashMap<{}, {}>", self.rust_ty(k), self.rust_ty(v)),
            Ty::Tuple(parts) => {
                let inner = parts.iter().map(|p| self.rust_ty(p)).collect::<Vec<_>>().join(", ");
                if parts.len() == 1 {
                    format!("({},)", inner)
                } else {
                    format!("({})", inner)
                }
            }
            Ty::Option(inner) => format!("Option<{}>", self.rust_ty(inner)),
            // A first-class function value lowers to a reference-counted boxed
            // closure `Rc<dyn Fn(A, B) -> R>`. `Rc` is `Clone`, so it round-trips
            // through pyrst's value semantics (clone-on-use = a cheap refcount
            // bump that shares the same callable) and is storable in a list/dict,
            // passable as an argument, and returnable. A `() -> R` return is
            // omitted in Rust only for `()`, but writing `-> ()` is also valid and
            // keeps the formatting uniform.
            Ty::Func(args, ret) => {
                let arg_strs = args.iter().map(|a| self.rust_ty(a)).collect::<Vec<_>>().join(", ");
                format!("::std::rc::Rc<dyn Fn({}) -> {}>", arg_strs, self.rust_ty(ret))
            }
            Ty::Class(n, args) => {
                // Generics v2 (generic CLASSES): a parametrized instance type
                // `Ty::Class("Box", [Int])` lowers to `Box<i64>` — the class's
                // Rust struct/impl carry a `<T, ..>` clause (see `emit_class`), and
                // every arg position lowers recursively. A generic class is never a
                // polymorphic base (generic-class inheritance is out of scope), so
                // the two paths don't overlap; the args-empty branch below is the
                // unchanged legacy lowering (plain `n` / companion enum `n__`).
                if !args.is_empty() {
                    let arg_strs = args.iter().map(|a| self.rust_ty(a)).collect::<Vec<_>>().join(", ");
                    return format!("{}<{}>", n, arg_strs);
                }
                // (EPIC-5 C2-2b-i) Polymorphism activation. A class that is a
                // polymorphic base (has ≥1 subclass in this unit) lowers to its
                // companion enum `n__` — emitted by emit_companion_enum with
                // method/field/dunder dispatch — for EVERY param/return/field/
                // var/element position. A leaf or non-subclassed class stays its
                // plain value-struct `n`. The C2-2b-i wrapping (at the 3 former
                // gate sites + list literals) and field-read lowering keep the
                // emitted Rust well-typed against this `n__` slot.
                if self.is_polymorphic_base(n) {
                    format!("{}__", n)
                } else {
                    n.clone()
                }
            }
            Ty::File => "PyFile".into(),
            // Generics v1: a bound type variable lowers to its bare Rust generic
            // parameter name (e.g. `T`). The enclosing `fn` declares it as
            // `<T: Clone>` (see `emit_func`), so the name is in scope wherever a
            // param/return/local of type `T` is emitted; monomorphization at the
            // call site fills it in.
            Ty::TypeVar(n) => n.clone(),
            Ty::Unknown => "()".into(),
        }
    }
}
