/**
 * Icons a project folder can have (24×24 line icons, drawn with the stroke of
 * the surrounding text). Shapes after Lucide (ISC license). `body` is SVG
 * markup inside the <svg>; ids are also what `folder_kind` (Rust) suggests.
 */
export type FolderIconDef = { id: string; label: string; body: string };

const p = (...d: string[]) => d.map((x) => `<path d="${x}"/>`).join('');

export const FOLDER_ICONS: FolderIconDef[] = [
  { id: 'folder', label: 'Folder', body: p('M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z') },
  { id: 'code', label: 'Code', body: p('m16 18 6-6-6-6', 'm8 6-6 6 6 6') },
  { id: 'terminal', label: 'Terminal', body: p('m4 17 6-6-6-6', 'M12 19h8') },
  { id: 'web', label: 'Web', body: '<circle cx="12" cy="12" r="10"/>' + p('M2 12h20', 'M12 2a15 15 0 0 1 0 20', 'M12 2a15 15 0 0 0 0 20') },
  { id: 'mobile', label: 'Mobile app', body: '<rect x="5" y="2" width="14" height="20" rx="2"/>' + p('M12 18h.01') },
  { id: 'database', label: 'Database', body: '<ellipse cx="12" cy="5" rx="9" ry="3"/>' + p('M3 5v14a9 3 0 0 0 18 0V5', 'M3 12a9 3 0 0 0 18 0') },
  { id: 'server', label: 'Server', body: '<rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/>' + p('M6 6h.01', 'M6 18h.01') },
  { id: 'cloud', label: 'Cloud', body: p('M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z') },
  { id: 'ai', label: 'AI / ML', body: '<rect x="3" y="8" width="18" height="12" rx="2"/>' + p('M12 8V4H8', 'M2 14h1', 'M21 14h1', 'M9 13v2', 'M15 13v2') },
  { id: 'chip', label: 'Hardware', body: '<rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/>' + p('M9 1v3', 'M15 1v3', 'M9 20v3', 'M15 20v3', 'M20 9h3', 'M20 14h3', 'M1 9h3', 'M1 14h3') },
  { id: 'game', label: 'Game', body: '<rect x="2" y="6" width="20" height="12" rx="2"/>' + p('M6 12h4', 'M8 10v4', 'M15 13h.01', 'M18 11h.01') },
  { id: 'design', label: 'Design', body: p('m12 19 7-7 3 3-7 7-3-3z', 'm18 13-1.5-7.5L2 2l3.5 14.5L13 18l5-5z', 'm2 2 7.586 7.586') + '<circle cx="11" cy="11" r="2"/>' },
  { id: 'science', label: 'Lab', body: p('M9 3h6', 'M10 3v6L4.5 19a1.5 1.5 0 0 0 1.3 2h12.4a1.5 1.5 0 0 0 1.3-2L14 9V3', 'M7 15h10') },
  { id: 'atom', label: 'Physics', body: '<circle cx="12" cy="12" r="1.5"/><ellipse cx="12" cy="12" rx="10" ry="4" transform="rotate(60 12 12)"/><ellipse cx="12" cy="12" rx="10" ry="4" transform="rotate(-60 12 12)"/><ellipse cx="12" cy="12" rx="10" ry="4"/>' },
  { id: 'dna', label: 'Biology', body: p('M5 2c0 5 14 5 14 10S5 17 5 22', 'M19 2c0 5-14 5-14 10s14 5 14 10', 'M8 5h8', 'M8 19h8', 'M10 12h4') },
  { id: 'microscope', label: 'Microscope', body: p('M6 18h8', 'M3 22h18', 'M14 22a7 7 0 1 0 0-14h-1', 'M9 14h2', 'M9 12a2 2 0 0 1-2-2V6h6v4a2 2 0 0 1-2 2Z', 'M12 6V3a1 1 0 0 0-1-1H9a1 1 0 0 0-1 1v3') },
  { id: 'chart', label: 'Data', body: p('M3 3v18h18', 'M7 16v-4', 'M12 16V8', 'M17 16v-7') },
  { id: 'math', label: 'Math', body: p('M18 7V4H6l6 8-6 8h12v-3') },
  { id: 'book', label: 'Notes', body: p('M4 19.5A2.5 2.5 0 0 1 6.5 17H20V2H6.5A2.5 2.5 0 0 0 4 4.5z', 'M20 17v5H6.5A2.5 2.5 0 0 1 4 19.5') },
  { id: 'doc', label: 'Paper', body: p('M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z', 'M14 2v6h6', 'M8 13h8', 'M8 17h5') },
  { id: 'git', label: 'Git', body: '<circle cx="6" cy="18" r="3"/><circle cx="18" cy="6" r="3"/>' + p('M6 3v12', 'M18 9a9 9 0 0 1-9 9') },
  { id: 'lock', label: 'Security', body: '<rect x="4" y="11" width="16" height="10" rx="2"/>' + p('M8 11V7a4 4 0 0 1 8 0v4') },
  { id: 'rocket', label: 'Launch', body: p('M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z', 'm12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z', 'M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0', 'M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5') },
  { id: 'tools', label: 'Tools', body: p('M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94z') },
];

/** Colors readable on both the light and the dark ground ('' = the text color). */
export const FOLDER_COLORS: { id: string; label: string; value: string }[] = [
  { id: '', label: 'Default', value: '' },
  { id: 'amber', label: 'Amber', value: '#e39b4b' },
  { id: 'red', label: 'Red', value: '#e5484d' },
  { id: 'pink', label: 'Pink', value: '#e0508a' },
  { id: 'purple', label: 'Purple', value: '#8b5cf6' },
  { id: 'blue', label: 'Blue', value: '#3b82f6' },
  { id: 'teal', label: 'Teal', value: '#0ea5a4' },
  { id: 'green', label: 'Green', value: '#22a06b' },
  { id: 'slate', label: 'Slate', value: '#8b93a1' },
];

export const iconById = (id: string) => FOLDER_ICONS.find((i) => i.id === id) ?? FOLDER_ICONS[0];
export const colorById = (id: string) => FOLDER_COLORS.find((c) => c.id === id)?.value ?? '';
