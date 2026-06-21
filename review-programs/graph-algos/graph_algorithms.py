# Graph algorithms over a directed adjacency-list graph.
#
# Implements, all with deterministic output:
#   - adjacency-list construction (dict[str, list[str]])
#   - breadth-first search (queue via list.pop(0))
#   - depth-first search (recursive + iterative stack)
#   - shortest path by edge count (BFS + parent map -> path reconstruction)
#   - cycle detection (DFS three-coloring with two sets)
#   - topological sort (Kahn's algorithm over in-degrees)
#
# pyrst is a statically typed subset of Python that compiles to Rust. Because
# the compiler uses Rust value/move semantics, this program threads the graph
# as a plain dict[str, list[str]] through free functions (function arguments
# are cloned at the call site), rather than storing mutable collections inside
# a class. A class (SearchResult) is still used, as an immutable value object
# with scalar fields and methods -- the shape pyrst classes handle cleanly.
#
# Conventions used for determinism:
#   - neighbors are always traversed in sorted() order
#   - node iteration uses sorted(g.keys()) (HashMap order is otherwise random)


# ---------------------------------------------------------------------------
# Graph construction and accessors. The graph is a dict mapping each node name
# to a list of successor node names. We also keep a separate set of all node
# names so that nodes with no outgoing edges still appear.
# ---------------------------------------------------------------------------
def new_graph() -> dict[str, list[str]]:
    g: dict[str, list[str]] = {}
    return g


def add_node(g: dict[str, list[str]], name: str) -> dict[str, list[str]]:
    if name not in g:
        empty: list[str] = []
        g[name] = empty
    return g


def add_edge(g: dict[str, list[str]], src: str, dst: str) -> dict[str, list[str]]:
    # ensure both endpoints exist
    if src not in g:
        e1: list[str] = []
        g[src] = e1
    if dst not in g:
        e2: list[str] = []
        g[dst] = e2
    succ: list[str] = g[src]
    if dst not in succ:
        succ.append(dst)
        g[src] = succ
    return g


def neighbors(g: dict[str, list[str]], name: str) -> list[str]:
    if name in g:
        return g[name]
    empty: list[str] = []
    return empty


def sorted_neighbors(g: dict[str, list[str]], name: str) -> list[str]:
    return sorted(neighbors(g, name))


def node_names(g: dict[str, list[str]]) -> list[str]:
    names: list[str] = []
    for k in g.keys():
        names.append(k)
    return sorted(names)


def order(g: dict[str, list[str]]) -> int:
    # number of nodes
    return len(g)


def size(g: dict[str, list[str]]) -> int:
    # number of directed edges
    total: int = 0
    for name in node_names(g):
        succ: list[str] = neighbors(g, name)
        total = total + len(succ)
    return total


def in_degrees(g: dict[str, list[str]]) -> dict[str, int]:
    degree: dict[str, int] = {}
    for name in node_names(g):
        degree[name] = 0
    for name in node_names(g):
        for nxt in neighbors(g, name):
            current: int = degree[nxt]
            degree[nxt] = current + 1
    return degree


# ---------------------------------------------------------------------------
# An immutable result value object describing one shortest-path query.
# Demonstrates pyrst classes: scalar fields, __init__, and methods that derive
# new values. (Classes that *hold* mutable collections are avoided because the
# compiler's value semantics make reading those fields back out awkward.)
# ---------------------------------------------------------------------------
class SearchResult:
    start: str
    goal: str
    found: bool
    hops: int

    def __init__(self, start: str, goal: str, found: bool, hops: int) -> None:
        self.start = start
        self.goal = goal
        self.found = found
        self.hops = hops

    def is_reachable(self) -> bool:
        return self.found

    def summary(self) -> str:
        if self.found:
            return f"{self.start} -> {self.goal}: reachable in {self.hops} hop(s)"
        return f"{self.start} -> {self.goal}: unreachable"


# ---------------------------------------------------------------------------
# Breadth-first search. Returns visitation order from `start`. Uses a list as
# a FIFO queue (append to enqueue, pop(0) to dequeue) and a set of seen nodes.
# ---------------------------------------------------------------------------
def bfs_order(g: dict[str, list[str]], start: str) -> list[str]:
    order_out: list[str] = []
    seen: set[str] = set()
    queue: list[str] = [start]
    seen.add(start)
    while len(queue) > 0:
        current: str = queue.pop(0)
        for nxt in sorted_neighbors(g, current):
            if nxt not in seen:
                seen.add(nxt)
                queue.append(nxt)
        # record `current` last: in pyrst, list.append() moves its argument,
        # so `current` must not be used again after being appended.
        order_out.append(current)
    return order_out


# ---------------------------------------------------------------------------
# Recursive DFS.
#
# pyrst passes arguments by value (the call site clones them), and tuple-unpack
# assignment to an existing variable currently shadows rather than reassigns,
# so the usual Python idiom of threading a shared `visited` set through the
# recursion does not work. Instead each call returns the pre-order traversal of
# the subtree rooted at `current` (a single list -- single return values DO
# propagate). A node reachable by several paths therefore appears more than
# once in the raw concatenation, so dfs_order() dedups by first occurrence,
# which preserves correct DFS pre-order.
# ---------------------------------------------------------------------------
def dfs_subtree(g: dict[str, list[str]], current: str) -> list[str]:
    order_out: list[str] = [current]
    for nxt in sorted_neighbors(g, current):
        segment: list[str] = dfs_subtree(g, nxt)
        for node in segment:
            order_out.append(node)
    return order_out


def dedup_keep_first(items: list[str]) -> list[str]:
    seen: set[str] = set()
    out: list[str] = []
    for item in items:
        if item not in seen:
            seen.add(item)
            out.append(item)
    return out


def dfs_order(g: dict[str, list[str]], start: str) -> list[str]:
    # Note: dfs_subtree must not be called on a cyclic graph (it would not
    # terminate); this demo only runs it on the acyclic dependency graph.
    raw: list[str] = dfs_subtree(g, start)
    return dedup_keep_first(raw)


# ---------------------------------------------------------------------------
# Iterative DFS using an explicit stack (list append/pop). Neighbors are pushed
# in reverse-sorted order so that they pop in sorted order, matching the
# recursive variant's output.
# ---------------------------------------------------------------------------
def dfs_iterative(g: dict[str, list[str]], start: str) -> list[str]:
    visited: set[str] = set()
    order_out: list[str] = []
    stack: list[str] = [start]
    while len(stack) > 0:
        current: str = stack.pop()
        if current in visited:
            continue
        visited.add(current)
        # push successors in reverse-sorted order -> sorted pop order
        succ: list[str] = sorted_neighbors(g, current)
        reversed_succ: list[str] = succ[::-1]
        for nxt in reversed_succ:
            if nxt not in visited:
                stack.append(nxt)
        order_out.append(current)
    return order_out


# ---------------------------------------------------------------------------
# Shortest path by edge count, via BFS. Records each node's discoverer in a
# parent map, then walks parents backward from `goal`. Returns the empty list
# when no path exists.
# ---------------------------------------------------------------------------
def shortest_path(g: dict[str, list[str]], start: str, goal: str) -> list[str]:
    if start == goal:
        single: list[str] = [start]
        return single

    parent: dict[str, str] = {}
    seen: set[str] = set()
    queue: list[str] = [start]
    seen.add(start)
    found: bool = False

    while len(queue) > 0:
        current: str = queue.pop(0)
        if current == goal:
            found = True
            break
        for nxt in sorted_neighbors(g, current):
            if nxt not in seen:
                seen.add(nxt)
                parent[nxt] = current
                queue.append(nxt)

    empty: list[str] = []
    if not found:
        return empty

    # walk backward from goal to start, then reverse
    reverse_path: list[str] = [goal]
    cursor: str = goal
    while cursor != start:
        # append parent[cursor], then advance cursor by re-reading the map,
        # so the appended value is not the loop-carried variable (move-safe).
        reverse_path.append(parent[cursor])
        cursor = parent[cursor]
    return reverse_path[::-1]


# ---------------------------------------------------------------------------
# Cycle detection via DFS three-coloring expressed with two sets:
#   in_progress  -> "gray", currently on the recursion stack
#   done         -> "black", fully explored
# An edge into an in-progress node closes a cycle.
# ---------------------------------------------------------------------------
def cycle_from(g: dict[str, list[str]], current: str, in_progress: set[str], done: set[str]) -> bool:
    in_progress.add(current)
    for nxt in sorted_neighbors(g, current):
        if nxt in in_progress:
            return True
        if nxt not in done:
            if cycle_from(g, nxt, in_progress, done):
                return True
    in_progress.discard(current)
    done.add(current)
    return False


def has_cycle(g: dict[str, list[str]]) -> bool:
    in_progress: set[str] = set()
    done: set[str] = set()
    for node in node_names(g):
        if node not in done:
            if cycle_from(g, node, in_progress, done):
                return True
    return False


# ---------------------------------------------------------------------------
# Topological sort via Kahn's algorithm. Repeatedly removes a zero-in-degree
# node (smallest name first, for determinism) and decrements its successors.
# Raises ValueError if the graph is cyclic.
# ---------------------------------------------------------------------------
def topological_sort(g: dict[str, list[str]]) -> list[str]:
    degree: dict[str, int] = in_degrees(g)

    ready: list[str] = []
    for node in node_names(g):
        if degree[node] == 0:
            ready.append(node)

    result: list[str] = []
    while len(ready) > 0:
        ready = sorted(ready)
        current: str = ready.pop(0)
        for nxt in sorted_neighbors(g, current):
            d: int = degree[nxt]
            degree[nxt] = d - 1
            if degree[nxt] == 0:
                ready.append(nxt)
        result.append(current)

    if len(result) != order(g):
        raise ValueError("graph has a cycle; no topological ordering exists")
    return result


# ---------------------------------------------------------------------------
# Scenario builders.
# ---------------------------------------------------------------------------
def build_dependency_graph() -> dict[str, list[str]]:
    # A small build-dependency DAG. Edge A -> B means "A must precede B".
    g: dict[str, list[str]] = new_graph()
    g = add_edge(g, "core", "lexer")
    g = add_edge(g, "core", "parser")
    g = add_edge(g, "lexer", "parser")
    g = add_edge(g, "parser", "ast")
    g = add_edge(g, "parser", "typeck")
    g = add_edge(g, "ast", "typeck")
    g = add_edge(g, "ast", "codegen")
    g = add_edge(g, "typeck", "codegen")
    g = add_edge(g, "codegen", "driver")
    g = add_node(g, "docs")  # isolated node
    return g


def build_cyclic_graph() -> dict[str, list[str]]:
    # Deliberately introduce a cycle: x -> y -> z -> x.
    g: dict[str, list[str]] = new_graph()
    g = add_edge(g, "x", "y")
    g = add_edge(g, "y", "z")
    g = add_edge(g, "z", "x")
    g = add_edge(g, "z", "w")
    return g


def join(items: list[str]) -> str:
    out: str = ""
    for i, item in enumerate(items):
        if i > 0:
            out = out + " -> "
        out = out + item
    return out


def main() -> None:
    print("=== Graph Algorithms Demo ===")

    g: dict[str, list[str]] = build_dependency_graph()
    print(f"nodes: {order(g)}")
    print(f"edges: {size(g)}")

    print("=== Adjacency (sorted) ===")
    for node in node_names(g):
        succ: list[str] = sorted_neighbors(g, node)
        print(f"{node}: {join(succ)}")

    print("=== BFS from core ===")
    bfs: list[str] = bfs_order(g, "core")
    print(join(bfs))
    print(f"reached: {len(bfs)}")

    print("=== DFS (recursive) from core ===")
    dfs_r: list[str] = dfs_order(g, "core")
    print(join(dfs_r))

    print("=== DFS (iterative) from core ===")
    dfs_i: list[str] = dfs_iterative(g, "core")
    print(join(dfs_i))

    print("=== Shortest path core -> driver ===")
    path1: list[str] = shortest_path(g, "core", "driver")
    r1: SearchResult = SearchResult("core", "driver", len(path1) > 0, len(path1) - 1)
    if r1.is_reachable():
        print(join(path1))
    print(r1.summary())

    print("=== Shortest path driver -> core ===")
    path2: list[str] = shortest_path(g, "driver", "core")
    found2: bool = len(path2) > 0
    hops2: int = 0
    if found2:
        hops2 = len(path2) - 1
    r2: SearchResult = SearchResult("driver", "core", found2, hops2)
    if r2.is_reachable():
        print(join(path2))
    print(r2.summary())

    print("=== In-degrees ===")
    degree: dict[str, int] = in_degrees(g)
    for node in node_names(g):
        d: int = degree[node]
        print(f"{node}: {d}")

    print("=== Cycle check (DAG) ===")
    print(has_cycle(g))

    print("=== Topological sort (DAG) ===")
    topo: list[str] = topological_sort(g)
    print(join(topo))

    print("=== Cyclic graph ===")
    c: dict[str, list[str]] = build_cyclic_graph()
    print(f"nodes: {order(c)}")
    print(f"has_cycle: {has_cycle(c)}")

    print("=== Topological sort (cyclic, should fail) ===")
    try:
        bad: list[str] = topological_sort(c)
        print(join(bad))
    except ValueError as e:
        msg: str = e
        print(f"rejected: {msg}")

    print("=== BFS on cyclic graph from x ===")
    cyc_bfs: list[str] = bfs_order(c, "x")
    print(join(cyc_bfs))

    print("=== Done ===")
