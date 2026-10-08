import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import type { SheetData, Workbook } from '../api/types';

/** Height of one row. It is fixed on purpose: only a known height lets the view skip the rows that
 *  are off screen, and that is the whole difference between a table that scrolls and one that does
 *  not. A long value is cut with an ellipsis and shown in full on hover. */
const ROW_HEIGHT = 30;
/** Rows kept above and below the window so that a fast scroll does not show empty space. */
const OVERSCAN = 8;

const MIN_WIDTH = 80;
const MAX_WIDTH = 320;
/** Rows looked at when guessing how wide a column has to be. */
const WIDTH_SAMPLE = 200;

export interface SheetTarget {
  sheet: number;
  /** Index in `rows` of that sheet, counting the header row of a delimited file. */
  row: number;
}

/** Which row of which sheet a line of the table's text form points at. This mirrors
 *  `sheet::to_lines` on the server: a named sheet writes its name first, and a sheet that was cut
 *  writes a note after its rows. The two must be changed together. */
export function sheetTarget(book: Workbook, line: number): SheetTarget | undefined {
  let seen = 0;
  for (let s = 0; s < book.sheets.length; s++) {
    const sheet = book.sheets[s]!;
    if (sheet.name) {
      seen += 1;
      if (line === seen) return { sheet: s, row: 0 };
    }
    if (line <= seen + sheet.rows.length) return { sheet: s, row: line - seen - 1 };
    seen += sheet.rows.length + (sheet.truncated ? 1 : 0);
  }
  return undefined;
}

/** The spreadsheet name of a column: A, B ... Z, AA, AB. */
function columnName(index: number): string {
  let name = '';
  for (let n = index; n >= 0; n = Math.floor(n / 26) - 1) {
    name = String.fromCharCode(65 + (n % 26)) + name;
  }
  return name;
}

/** Column widths from the content of the first rows. They have to be decided in advance: a fixed
 *  layout is what keeps the columns from jumping as rows are swapped under the scroll. */
function widthsOf(rows: string[][], count: number): number[] {
  const widths = new Array<number>(count).fill(MIN_WIDTH);
  for (const row of rows.slice(0, WIDTH_SAMPLE)) {
    for (let i = 0; i < count; i++) {
      const length = row[i]?.length ?? 0;
      widths[i] = Math.max(widths[i]!, Math.min(MAX_WIDTH, 24 + length * 7.2));
    }
  }
  return widths.map(Math.round);
}

function Rows({ rows, from, to, first, widths, hit }: {
  rows: string[][];
  from: number;
  to: number;
  first: number;
  widths: number[];
  hit: number;
}) {
  const out = [];
  for (let r = from; r < to; r++) {
    const row = rows[r] ?? [];
    out.push(
      <tr key={r} class={r === hit ? 'search-target' : undefined}>
        <td class="sheet-no">{r + first}</td>
        {widths.map((_, i) => (
          <td key={i} title={(row[i]?.length ?? 0) > 24 ? row[i] : undefined}>{row[i] ?? ''}</td>
        ))}
      </tr>,
    );
  }
  return <>{out}</>;
}

/** A table read by the server: sheets as tabs. Only the rows in view are in the document, the rest
 *  of the height is held by two empty rows, so a sheet of twenty thousand rows costs about as much
 *  as a screenful. A delimited file has a header row and is shown with it; a sheet of a workbook is
 *  shown the way Excel shows it, with lettered columns and every row numbered. */
export function SheetView({ book, target }: { book: Workbook; target?: SheetTarget }) {
  const [active, setActive] = useState(target?.sheet ?? 0);
  const [start, setStart] = useState(0);
  const [count, setCount] = useState(40);
  const box = useRef<HTMLDivElement>(null);
  const sheet: SheetData | undefined = book.sheets[active];

  const { rows, head, firstNumber, widths } = useMemo(() => {
    const all = sheet?.rows ?? [];
    const columns = all.reduce((max, row) => Math.max(max, row.length), 0);
    return {
      rows: book.header ? all.slice(1) : all,
      head: book.header ? all[0] : null,
      firstNumber: book.header ? 2 : 1,
      widths: widthsOf(all, columns),
    };
  }, [sheet, book.header]);

  // A hit from the search points at a row of a sheet, which may not be the one on screen.
  useEffect(() => {
    if (target) setActive(target.sheet);
  }, [target]);

  // The row the hit is on, in the numbering of the rows actually drawn.
  const hit = target && target.sheet === active ? target.row - (book.header ? 1 : 0) : -1;

  // The window follows the scroll; it is also recomputed when the sheet or the size changes.
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const update = () => {
      setStart(Math.max(0, Math.floor(el.scrollTop / ROW_HEIGHT) - OVERSCAN));
      setCount(Math.ceil(el.clientHeight / ROW_HEIGHT) + OVERSCAN * 2);
    };
    // A hit is scrolled to the middle of the view, everything else starts at the top
    el.scrollTop =
      hit >= 0 ? Math.max(0, hit * ROW_HEIGHT - el.clientHeight / 2 + ROW_HEIGHT / 2) : 0;
    update();
    el.addEventListener('scroll', update, { passive: true });
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => {
      el.removeEventListener('scroll', update);
      observer.disconnect();
    };
  }, [active, book]);

  if (!sheet) return <p class="doc-status muted">The table has no sheets</p>;

  const end = Math.min(rows.length, start + count);
  const above = start * ROW_HEIGHT;
  const below = (rows.length - end) * ROW_HEIGHT;

  return (
    <div class="sheet">
      {book.sheets.length > 1 && (
        <div class="sheet-tabs" role="tablist">
          {book.sheets.map((s, i) => (
            <button
              key={s.name}
              type="button"
              role="tab"
              aria-selected={i === active}
              class={`sheet-tab ${i === active ? 'sheet-tab--on' : ''}`}
              onClick={() => setActive(i)}
            >
              {s.name}
            </button>
          ))}
        </div>
      )}

      <div class="sheet-scroll" ref={box}>
        <table class={`sheet-table ${book.header ? '' : 'sheet-table--grid'}`}>
          <colgroup>
            <col class="sheet-no-col" />
            {widths.map((w, i) => <col key={i} style={{ width: `${w}px` }} />)}
          </colgroup>
          <thead>
            <tr>
              <th class="sheet-no" />
              {widths.map((_, i) => (
                <th key={i} title={head?.[i]}>{head ? (head[i] ?? '') : columnName(i)}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {above > 0 && <tr class="sheet-gap" style={{ height: `${above}px` }} />}
            <Rows rows={rows} from={start} to={end} first={firstNumber} widths={widths} hit={hit} />
            {below > 0 && <tr class="sheet-gap" style={{ height: `${below}px` }} />}
          </tbody>
        </table>
      </div>

      <p class="sheet-note muted">
        {rows.length} {rows.length === 1 ? 'row' : 'rows'}
        {(sheet.truncated || book.truncated) && (
          <>
            {sheet.truncated && ', the sheet is shown in part'}
            {book.truncated && ', some sheets were left out'}
            {'. Open the file itself to see all of it.'}
          </>
        )}
      </p>
    </div>
  );
}
