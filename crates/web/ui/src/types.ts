/** A searchable item entry for GlobalSearch in AppShell. */
export interface SearchableEntry {
  id: string; // unique key for the item
  sectionId: string; // which section this item belongs to
  sectionLabel: string; // display name for the section
  title: string; // primary display text
  subtitle?: string; // secondary text
  searchText: string; // full searchable text
}
