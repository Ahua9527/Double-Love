import { Clock3, FolderOpen, Settings2 } from "lucide-react";

export type StudioScreen = "library" | "editor" | "tasks" | "settings";
export type TaskBadgeTone = "active" | "attention";

interface SidebarProps {
    screen: StudioScreen;
    taskCount: number;
    taskTone: TaskBadgeTone;
    onNavigate: (screen: StudioScreen) => void;
    onOpenSettings?: () => void;
}

const ITEMS: Array<{
    id: "library" | "tasks";
    label: string;
    icon: typeof FolderOpen;
}> = [
    { id: "library", label: "我的项目", icon: FolderOpen },
    { id: "tasks", label: "后台任务", icon: Clock3 },
];

export function Sidebar({
    screen,
    taskCount,
    taskTone,
    onNavigate,
    onOpenSettings,
}: SidebarProps) {
    return (
        <aside className="studio-sidebar" aria-label="工作区导航">
            <nav className="studio-nav-list">
                {ITEMS.map(({ id, label, icon: Icon }) => (
                    <button
                        key={id}
                        type="button"
                        aria-current={screen === id ? "page" : undefined}
                        className={screen === id ? "is-active" : ""}
                        onClick={() => onNavigate(id)}
                    >
                        <Icon size={16} />
                        <span>{label}</span>
                        {id === "tasks" && taskCount > 0 && (
                            <b
                                className={`studio-task-badge is-${taskTone}`}
                                aria-label={`${taskCount} 个后台任务`}
                            >
                                {taskCount}
                            </b>
                        )}
                    </button>
                ))}
            </nav>
            <button
                type="button"
                aria-current={screen === "settings" ? "page" : undefined}
                className={`studio-sidebar-settings${screen === "settings" ? " is-active" : ""}`}
                onClick={() =>
                    onOpenSettings ? onOpenSettings() : onNavigate("settings")
                }
            >
                <Settings2 size={16} />
                <span>设置</span>
            </button>
        </aside>
    );
}
