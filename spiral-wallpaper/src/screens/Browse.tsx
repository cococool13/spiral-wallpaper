import { useEffect, useRef, useState } from "react";
import { WallpaperTile } from "../components/WallpaperTile";
import { useDebounce } from "../hooks/useDebounce";
import { errorCopy, SOURCES } from "../sources";
import type { Wallpaper } from "../sources/types";

// Wallhaven-only category filters — other sources have no category axis.
const CHIPS = [
  { label: "Toplist", categories: "111" },
  { label: "General", categories: "100" },
  { label: "Anime", categories: "010" },
  { label: "People", categories: "001" },
] as const;

// The strictest source limit (Wallhaven, ~45 requests/minute) sets the pace.
const SEARCH_DEBOUNCE_MS = 500;

type Status = "idle" | "loading" | "ready" | "error";

export function Browse() {
  const [sourceIndex, setSourceIndex] = useState(0);
  const [query, setQuery] = useState("");
  const [chipIndex, setChipIndex] = useState(0);
  const [status, setStatus] = useState<Status>("idle");
  const [items, setItems] = useState<Wallpaper[]>([]);
  const [pageNum, setPageNum] = useState(1);
  const [lastPage, setLastPage] = useState(1);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string>();
  // Privacy pillar: no network until the user acts.
  const [touched, setTouched] = useState(false);
  const requestId = useRef(0);

  const debouncedQuery = useDebounce(query, SEARCH_DEBOUNCE_MS);
  const gridRef = useRef<HTMLDivElement>(null);

  // Arrow-key navigation between tiles (Enter activates natively).
  function onGridKeyDown(e: React.KeyboardEvent) {
    const handled = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"];
    if (!handled.includes(e.key) || !gridRef.current) return;
    const grid = gridRef.current;
    const buttons = Array.from(grid.querySelectorAll<HTMLButtonElement>(".tile button"));
    if (buttons.length === 0) return;

    const tiles = Array.from(grid.children) as HTMLElement[];
    const firstRowTop = tiles[0]?.offsetTop;
    const columns = Math.max(1, tiles.filter((t) => t.offsetTop === firstRowTop).length);

    const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const last = buttons.length - 1;
    let next: number;
    switch (e.key) {
      case "ArrowLeft":
        next = Math.max(0, current - 1);
        break;
      case "ArrowRight":
        next = Math.min(last, current + 1);
        break;
      case "ArrowUp":
        next = Math.max(0, current - columns);
        break;
      case "ArrowDown":
        next = Math.min(last, current < 0 ? 0 : current + columns);
        break;
      case "Home":
        next = 0;
        break;
      default:
        next = last;
    }
    buttons[next]?.focus();
    e.preventDefault();
  }

  const active = SOURCES[sourceIndex];

  useEffect(() => {
    if (!touched) return;
    const id = ++requestId.current;
    setStatus("loading");
    setError(undefined);
    active.api
      .search({
        query: debouncedQuery,
        categories: CHIPS[chipIndex].categories,
        sorting: debouncedQuery ? "relevance" : "toplist",
        page: 1,
      })
      .then((result) => {
        if (id !== requestId.current) return;
        setItems(result.items);
        setPageNum(result.page);
        setLastPage(result.lastPage);
        setStatus("ready");
      })
      .catch((e: unknown) => {
        if (id !== requestId.current) return;
        setError(errorCopy(e, active.label));
        setStatus("error");
      });
  }, [debouncedQuery, chipIndex, sourceIndex, touched]);

  async function loadMore() {
    const id = requestId.current;
    setLoadingMore(true);
    setError(undefined);
    try {
      const result = await active.api.search({
        query: debouncedQuery,
        categories: CHIPS[chipIndex].categories,
        sorting: debouncedQuery ? "relevance" : "toplist",
        page: pageNum + 1,
      });
      if (id !== requestId.current) return; // a new search superseded this
      setItems((existing) => [...existing, ...result.items]);
      setPageNum(result.page);
      setLastPage(result.lastPage);
    } catch (e: unknown) {
      if (id === requestId.current) setError(errorCopy(e, active.label));
    } finally {
      setLoadingMore(false);
    }
  }

  return (
    <main className="browse">
      <input
        type="search"
        className="browse__search"
        aria-label="Search wallpapers"
        placeholder="search wallpapers"
        spellCheck={false}
        value={query}
        onChange={(e) => {
          setQuery(e.currentTarget.value);
          setTouched(true);
        }}
      />

      <div className="browse__filters">
        <div className="segmented" role="group" aria-label="Wallpaper source">
          {SOURCES.map((s, i) => (
            <button
              key={s.name}
              aria-pressed={i === sourceIndex}
              className={
                i === sourceIndex
                  ? "segmented__option segmented__option--active"
                  : "segmented__option"
              }
              onClick={() => {
                setSourceIndex(i);
                setTouched(true);
              }}
            >
              {s.label}
            </button>
          ))}
        </div>

        {active.name === "wallhaven" && (
          <div className="browse__chips">
            {CHIPS.map((chip, i) => (
              <button
                key={chip.label}
                className={i === chipIndex && touched ? "chip chip--active" : "chip"}
                onClick={() => {
                  setChipIndex(i);
                  setTouched(true);
                }}
              >
                {chip.label}
              </button>
            ))}
          </div>
        )}
      </div>

      {status === "idle" && (
        <section className="browse__empty" aria-label="No wallpapers loaded">
          <span className="browse__empty-eyebrow">Browse</span>
          <h1 className="browse__empty-title">Nothing loaded yet.</h1>
          <p className="browse__empty-copy">
            Search, or pick a category. Spiral makes no network requests until
            you act.
          </p>
        </section>
      )}

      {status === "loading" && (
        <section className="browse__empty" aria-label="Loading">
          <span className="browse__empty-eyebrow">Browse</span>
          <p className="browse__empty-copy">Fetching from {active.label}…</p>
        </section>
      )}

      {status === "error" && (
        <section className="browse__empty" aria-label="Search failed">
          <span className="browse__empty-eyebrow">Problem</span>
          <p className="browse__empty-copy">{error}</p>
        </section>
      )}

      {status === "ready" && items.length === 0 && (
        <section className="browse__empty" aria-label="No results">
          <span className="browse__empty-eyebrow">Browse</span>
          <h1 className="browse__empty-title">No results.</h1>
          <p className="browse__empty-copy">
            Nothing on {active.label} matches “{debouncedQuery}”. Try a broader
            search.
          </p>
        </section>
      )}

      {status === "ready" && items.length > 0 && (
        <div className="browse__scroll">
          <div className="browse__grid" role="list" ref={gridRef} onKeyDown={onGridKeyDown}>
            {items.map((wallpaper) => (
              <WallpaperTile key={wallpaper.id} wallpaper={wallpaper} source={active.api} />
            ))}
          </div>
          {error && <p className="browse__more-error">{error}</p>}
          {pageNum < lastPage && (
            <div className="browse__more">
              <button
                className="btn-glass btn-glass--secondary"
                onClick={loadMore}
                disabled={loadingMore}
              >
                {loadingMore ? "Loading…" : "Load more"}
              </button>
            </div>
          )}
        </div>
      )}
    </main>
  );
}
