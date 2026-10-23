/** One row of [`ReportBars`]: its label, the bar's share of the largest row, and value texts. */
export interface ReportBarRow {
  key: string
  label: string
  /** The "not set" and "direct" rows read as data about missing values, not as values. */
  muted?: boolean
  /** 0 to 1. */
  share: number
  values: string[]
}
