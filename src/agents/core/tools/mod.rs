// This module previously contained ProcessListTool, FileListTool, and ShellTool.
// All have been removed:
// - ProcessListTool / FileListTool: were only used by SystemAgent (now removed)
// - ShellTool: was only used by CodingAgent (now removed; coding via Skill Market)
//
// System capabilities are now provided directly in MainAgent (analyze_disk, clean_disk)
// or through the Skill registry (system.cleaner, desktop.organizer, etc.).
