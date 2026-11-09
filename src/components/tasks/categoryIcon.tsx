import { AppWindow, Archive, Disc3, File, FileText, Film, Folder, Image as ImageIcon, Music } from "lucide-react";

const ICONS: Record<string, typeof File> = {
  video: Film,
  audio: Music,
  images: ImageIcon,
  archives: Archive,
  programs: AppWindow,
  iso: Disc3,
  documents: FileText,
  other: File,
};

export function CategoryIcon({ id, className = "h-4 w-4" }: { id: string; className?: string }) {
  const I = ICONS[id] ?? Folder;
  return <I className={className} />;
}
