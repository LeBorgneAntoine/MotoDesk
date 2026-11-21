<script lang="ts" module>
  import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
  import Users from "@lucide/svelte/icons/users";
  import Wrench from "@lucide/svelte/icons/wrench";
  import Package from "@lucide/svelte/icons/package";
  import Receipt from "@lucide/svelte/icons/receipt";
  import Search from "@lucide/svelte/icons/search";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Plus from "@lucide/svelte/icons/plus";
  import Minus from "@lucide/svelte/icons/minus";
  import LogOut from "@lucide/svelte/icons/log-out";
  import TrendingUp from "@lucide/svelte/icons/trending-up";
  import TrendingDown from "@lucide/svelte/icons/trending-down";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Info from "@lucide/svelte/icons/info";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Clock from "@lucide/svelte/icons/clock";
  import Bike from "@lucide/svelte/icons/bike";
  import Car from "@lucide/svelte/icons/car";
  import FileText from "@lucide/svelte/icons/file-text";
  import CircleUser from "@lucide/svelte/icons/circle-user";
  import Settings from "@lucide/svelte/icons/settings";
  import Bell from "@lucide/svelte/icons/bell";
  import Inbox from "@lucide/svelte/icons/inbox";
  import Activity from "@lucide/svelte/icons/activity";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import CircleDot from "@lucide/svelte/icons/circle-dot";
  import Phone from "@lucide/svelte/icons/phone";
  import Mail from "@lucide/svelte/icons/mail";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import Zap from "@lucide/svelte/icons/zap";
  import Calendar from "@lucide/svelte/icons/calendar";
  import Hash from "@lucide/svelte/icons/hash";
  import Gauge from "@lucide/svelte/icons/gauge";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import ShoppingCart from "@lucide/svelte/icons/shopping-cart";
  import Tag from "@lucide/svelte/icons/tag";
  import Layers from "@lucide/svelte/icons/layers";
  import Boxes from "@lucide/svelte/icons/boxes";
  import LayoutTemplate from "@lucide/svelte/icons/layout-template";
  import Type from "@lucide/svelte/icons/type";
  import Image from "@lucide/svelte/icons/image";
  import Table from "@lucide/svelte/icons/table";
  import Square from "@lucide/svelte/icons/square";
  import Copy from "@lucide/svelte/icons/copy";
  import Archive from "@lucide/svelte/icons/archive";
  import Braces from "@lucide/svelte/icons/braces";
  import Eye from "@lucide/svelte/icons/eye";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Save from "@lucide/svelte/icons/save";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Redo2 from "@lucide/svelte/icons/redo-2";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import AlignLeft from "@lucide/svelte/icons/align-left";
  import AlignCenter from "@lucide/svelte/icons/align-center";
  import AlignRight from "@lucide/svelte/icons/align-right";
  import ArrowUpToLine from "@lucide/svelte/icons/arrow-up-to-line";
  import ArrowDownToLine from "@lucide/svelte/icons/arrow-down-to-line";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import Lock from "@lucide/svelte/icons/lock";
  import LockOpen from "@lucide/svelte/icons/lock-open";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Layers2 from "@lucide/svelte/icons/layers-2";
  import UsersRound from "@lucide/svelte/icons/users-round";
  import Building2 from "@lucide/svelte/icons/building-2";
  import Shield from "@lucide/svelte/icons/shield";
  import Upload from "@lucide/svelte/icons/upload";
  import Database from "@lucide/svelte/icons/database";
  import IdCard from "@lucide/svelte/icons/id-card";
  import Maximize2 from "@lucide/svelte/icons/maximize-2";
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import Funnel from "@lucide/svelte/icons/funnel";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import User from "@lucide/svelte/icons/user";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import List from "@lucide/svelte/icons/list";
  import History from "@lucide/svelte/icons/history";
  import Printer from "@lucide/svelte/icons/printer";
  import Download from "@lucide/svelte/icons/download";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import Ban from "@lucide/svelte/icons/ban";
  import Banknote from "@lucide/svelte/icons/banknote";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Play from "@lucide/svelte/icons/play";
  import Infinity_ from "@lucide/svelte/icons/infinity";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Scale from "@lucide/svelte/icons/scale";
  import Store from "@lucide/svelte/icons/store";
  import CreditCard from "@lucide/svelte/icons/credit-card";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Send from "@lucide/svelte/icons/send";
  import Bot from "@lucide/svelte/icons/bot";
  import Globe from "@lucide/svelte/icons/globe";
  import PanelLeft from "@lucide/svelte/icons/panel-left";

  const iconMap = {
    "layout-dashboard": LayoutDashboard,
    users: Users,
    wrench: Wrench,
    package: Package,
    receipt: Receipt,
    search: Search,
    "refresh-cw": RefreshCw,
    plus: Plus,
    minus: Minus,
    "log-out": LogOut,
    "trending-up": TrendingUp,
    "trending-down": TrendingDown,
    "chevron-left": ChevronLeft,
    "chevron-right": ChevronRight,
    "chevron-down": ChevronDown,
    check: Check,
    x: X,
    "alert-circle": CircleAlert,
    info: Info,
    "alert-triangle": TriangleAlert,
    "check-circle": CircleCheck,
    clock: Clock,
    bike: Bike,
    car: Car,
    "file-text": FileText,
    "circle-user": CircleUser,
    settings: Settings,
    bell: Bell,
    inbox: Inbox,
    activity: Activity,
    "more-horizontal": Ellipsis,
    "circle-dot": CircleDot,
    phone: Phone,
    mail: Mail,
    "map-pin": MapPin,
    pencil: Pencil,
    trash: Trash2,
    "user-plus": UserPlus,
    zap: Zap,
    calendar: Calendar,
    hash: Hash,
    gauge: Gauge,
    "sticky-note": StickyNote,
    "shopping-cart": ShoppingCart,
    tag: Tag,
    layers: Layers,
    boxes: Boxes,
    "layout-template": LayoutTemplate,
    type: Type,
    image: Image,
    table: Table,
    square: Square,
    copy: Copy,
    archive: Archive,
    braces: Braces,
    eye: Eye,
    "arrow-left": ArrowLeft,
    save: Save,
    undo: Undo2,
    redo: Redo2,
    "grip-vertical": GripVertical,
    "align-left":   AlignLeft,
    "align-center": AlignCenter,
    "align-right":  AlignRight,
    "to-front":     ArrowUpToLine,
    "to-back":      ArrowDownToLine,
    "layer-up":     ArrowUp,
    "layer-down":   ArrowDown,
    lock:           Lock,
    "lock-open":    LockOpen,
    "eye-off":      EyeOff,
    "layers-2":     Layers2,
    "users-round":  UsersRound,
    "building-2":   Building2,
    shield:         Shield,
    upload:         Upload,
    database:       Database,
    "id-card":      IdCard,
    "maximize":     Maximize2,
    "minimize":     Minimize2,
    filter:         Funnel,
    "chevron-up":   ChevronUp,
    "arrow-right":  ArrowRight,
    user:           User,
    "panel-right":  PanelRight,
    list:           List,
    history:        History,
    printer:        Printer,
    download:       Download,
    "rotate-cw":    RotateCw,
    ban:            Ban,
    banknote:       Banknote,
    "hard-drive":   HardDrive,
    "shield-check": ShieldCheck,
    play:           Play,
    infinity:       Infinity_,
    "book-open":    BookOpen,
    scale:          Scale,
    store:          Store,
    "credit-card":  CreditCard,
    sparkles: Sparkles,
    send: Send,
    bot: Bot,
    globe: Globe,
    "panel-left": PanelLeft,
  } as const;

  export type IconName = keyof typeof iconMap;
</script>

<script lang="ts">
  interface Props {
    name: IconName;
    size?: number | string;
    strokeWidth?: number | string;
    color?: string;
    title?: string;
    class?: string;
  }

  let {
    name,
    size = 16,
    strokeWidth = 1.75,
    color,
    title,
    class: className,
  }: Props = $props();

  const Component = $derived(iconMap[name]);
</script>

{#if Component}
  <Component
    {size}
    {strokeWidth}
    {color}
    {title}
    class={className}
    aria-hidden={title ? undefined : "true"}
  />
{/if}
