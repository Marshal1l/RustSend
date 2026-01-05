<script setup lang="ts">
import { ref, computed, onMounted, nextTick, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';// 新增：用于获取当前窗口实例

// 导入 ECharts (保持不变)
import * as echarts from 'echarts/core';
import {
    PieChart,
    BarChart
} from 'echarts/charts';
import {
    TitleComponent,
    TooltipComponent,
    LegendComponent,
    GridComponent
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';

// 注册 ECharts 必需组件 (保持不变)
echarts.use([
    PieChart,
    BarChart,
    TitleComponent,
    TooltipComponent,
    LegendComponent,
    GridComponent,
    CanvasRenderer
]);

// --- 窗口控制函数 ---
const appWindow = getCurrentWindow(); // 获取当前窗口实例

async function minimize() {
    await appWindow.minimize();
}

async function maximizeOrUnmaximize() {
    if (await appWindow.isMaximized()) {
        await appWindow.unmaximize();
    } else {
        await appWindow.maximize();
    }
}

async function closeApp() {
    await appWindow.close();
}

// --- 类型定义 ---
interface DirEntry {
    name: string;
    is_dir: boolean;
    is_parent?: boolean;
}

interface LocalDirEntry {
    name: string;
    is_dir: boolean;
    size: number;
    is_parent?: boolean;
}

// --- 状态管理 ---
const serverUrl = ref('http://127.0.0.1:50051'); 
const connectionStatus = ref('未连接');
const isConnected = ref(false);

const remoteFiles = ref<DirEntry[]>([]);
const currentRemotePath = ref('/');

const localFiles = ref<LocalDirEntry[]>([]);
const currentLocalPath = ref('/');
const checkedFiles = ref<LocalDirEntry[]>([]);

const uploadMessage = ref('');

// --- ECharts 实例引用 ---
const localTypeChartRef = ref<HTMLElement | null>(null);
const localSizeChartRef = ref<HTMLElement | null>(null);
const remoteTypeChartRef = ref<HTMLElement | null>(null);

let localTypeChart: echarts.ECharts | null = null;
let localSizeChart: echarts.ECharts | null = null;
let remoteTypeChart: echarts.ECharts | null = null;

// --- 计算属性 (保持不变) ---
const localTypeDistribution = computed(() => {
    let dirCount = 0;
    let fileCount = 0;
    localFiles.value.forEach(entry => {
        if (!entry.is_parent) {
            entry.is_dir ? dirCount++ : fileCount++;
        }
    });
    return [
        { value: dirCount, name: '目录' },
        { value: fileCount, name: '文件' }
    ].filter(item => item.value > 0);
});

const localSizeDistribution = computed(() => {
    const ranges = [
        { name: '< 1KB', min: 0, max: 1024 },
        { name: '1KB - 1MB', min: 1024, max: 1024 * 1024 },
        { name: '1MB - 100MB', min: 1024 * 1024, max: 100 * 1024 * 1024 },
        { name: '> 100MB', min: 100 * 1024 * 1024, max: Infinity }
    ];
    const counts = ranges.map(range => ({
        name: range.name,
        value: 0
    }));

    localFiles.value.forEach(entry => {
        if (!entry.is_parent && !entry.is_dir) {
            const size = entry.size;
            for (const range of ranges) {
                if (size >= range.min && size < range.max) {
                    const item = counts.find(c => c.name === range.name);
                    if (item) item.value++;
                    break;
                }
            }
        }
    });

    return {
        names: counts.map(c => c.name),
        values: counts.map(c => c.value)
    };
});

const remoteTypeDistribution = computed(() => {
    let dirCount = 0;
    let fileCount = 0;
    remoteFiles.value.forEach(entry => {
        if (!entry.is_parent) {
            entry.is_dir ? dirCount++ : fileCount++;
        }
    });
    return [
        { value: dirCount, name: '目录' },
        { value: fileCount, name: '文件' }
    ].filter(item => item.value > 0);
});

// --- 绘制所有图表 (保持不变) ---
function drawCharts() {
    // ... (图表绘制逻辑保持不变)
    // 本地类型饼图
    if (localTypeChartRef.value && !localTypeChart) {
        localTypeChart = echarts.init(localTypeChartRef.value);
    }
    if (localTypeChart) {
        localTypeChart.setOption({
            title: { text: '本地文件类型分布', left: 'center', textStyle: { fontSize: 13, color: '#333' } },
            tooltip: { trigger: 'item' },
            series: [{
                type: 'pie',
                radius: ['40%', '70%'],
                center: ['50%', '60%'],
                data: localTypeDistribution.value,
                itemStyle: { borderRadius: 8, borderColor: '#fff', borderWidth: 2 },
                label: { formatter: '{b}: {c}' },
                color: ['#36a2eb', '#ff6384']
            }]
        });
    }

    // 本地大小柱状图
    if (localSizeChartRef.value && !localSizeChart) {
        localSizeChart = echarts.init(localSizeChartRef.value);
    }
    if (localSizeChart) {
        localSizeChart.setOption({
            title: { text: '本地文件大小分布', left: 'center', textStyle: { fontSize: 13, color: '#333' } },
            tooltip: { trigger: 'axis' },
            grid: { left: '10%', right: '10%', bottom: '20%', containLabel: true },
            xAxis: { type: 'category', data: localSizeDistribution.value.names, axisLabel: { fontSize: 11, rotate: 30 } },
            yAxis: { type: 'value' },
            series: [{
                type: 'bar',
                data: localSizeDistribution.value.values,
                itemStyle: { color: '#4bc0c0', borderRadius: [4, 4, 0, 0] }
            }]
        });
    }

    // 远程类型饼图
    if (remoteTypeChartRef.value && !remoteTypeChart) {
        remoteTypeChart = echarts.init(remoteTypeChartRef.value);
    }
    if (remoteTypeChart) {
        remoteTypeChart.setOption({
            title: { text: '远程文件类型分布', left: 'center', textStyle: { fontSize: 13, color: '#333' } },
            tooltip: { trigger: 'item' },
            series: [{
                type: 'pie',
                radius: ['40%', '70%'],
                center: ['50%', '60%'],
                data: remoteTypeDistribution.value,
                itemStyle: { borderRadius: 8, borderColor: '#fff', borderWidth: 2 },
                label: { formatter: '{b}: {c}' },
                color: ['#ff9f40', '#ff6384']
            }]
        });
    }
}

// 数据变化时重绘 (保持不变)
watch([localTypeDistribution, localSizeDistribution, remoteTypeDistribution], () => {
    nextTick(drawCharts);
});

// 窗口变化自适应 (保持不变)
onMounted(() => {
    window.addEventListener('resize', () => {
        localTypeChart?.resize();
        localSizeChart?.resize();
        remoteTypeChart?.resize();
    });
});

// --- 其余函数 (保持不变) ---
function formatBytes(bytes: number, decimals = 2): string {
    if (bytes === 0) return '0 Bytes';
    const k = 1024;
    const dm = decimals < 0 ? 0 : decimals;
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i];
}

function getIconClass(entry: DirEntry | LocalDirEntry): string {
    if (entry.is_parent) return 'fas fa-level-up-alt';
    if (entry.is_dir) return 'fas fa-folder';
    return 'fas fa-file';
}

async function connectServer() {
    connectionStatus.value = '连接中...';
    try {
        uploadMessage.value = '';
        const message = await invoke('connect_server', { url: serverUrl.value });
        connectionStatus.value = message as string;
        isConnected.value = true;
        await listRemoteDir('/');
    } catch (error) {
        const errMsg = error as string;
        connectionStatus.value = `连接失败: ${errMsg}`;
        isConnected.value = false;
    }
}

async function listRemoteDir(path: string) {
    if (!isConnected.value) {
        uploadMessage.value = '请先连接服务器';
        return;
    }
    uploadMessage.value = `加载中: ${path}`;
    try {
        const entries = await invoke('list_remote_dir', { path }) as DirEntry[];
        const parentDir: DirEntry[] = path !== '/' ? [{ name: '.. (返回上级)', is_dir: true, is_parent: true }] : [];
        remoteFiles.value = parentDir.concat(entries.sort((a, b) => Number(b.is_dir) - Number(a.is_dir)));
        currentRemotePath.value = path;
        uploadMessage.value = '';
        nextTick(drawCharts);
    } catch (error) {
        uploadMessage.value = `失败: ${error}`;
    }
}

function handleRemoteClick(entry: DirEntry) {
    if (!entry.is_dir) {
        uploadMessage.value = `暂不支持下载: ${entry.name}`;
        return;
    }
    let parts = currentRemotePath.value.split('/').filter(Boolean);
    if (entry.is_parent) parts.pop();
    else parts.push(entry.name);
    const newPath = parts.length === 0 ? '/' : '/' + parts.join('/');
    listRemoteDir(newPath);
}

async function listLocalDir(path: string) {
    uploadMessage.value = `加载中: ${path}`;
    try {
        const result = await invoke('list_local_dir', { path }) as [LocalDirEntry[], string];
        const entries = result[0];
        const parentDir: LocalDirEntry[] = path !== '/' ? [{ name: '.. (返回上级)', is_dir: true, size: 0, is_parent: true }] : [];
        localFiles.value = parentDir.concat(entries.sort((a, b) => Number(b.is_dir) - Number(a.is_dir)));
        currentLocalPath.value = path;
        uploadMessage.value = '';
        nextTick(drawCharts);
    } catch (error) {
        uploadMessage.value = `失败: ${error}`;
    }
}

function handleLocalClick(entry: LocalDirEntry) {
    if (!entry.is_dir) return;
    let parts = currentLocalPath.value.split('/').filter(Boolean);
    if (entry.is_parent) parts.pop();
    else parts.push(entry.name);
    const newPath = parts.length === 0 ? '/' : '/' + parts.join('/');
    listLocalDir(newPath);
}

function handleFileCheck(entry: LocalDirEntry, event: Event) {
    if (entry.is_dir || entry.is_parent) return;
    const checked = (event.target as HTMLInputElement).checked;
    if (checked) {
        if (!checkedFiles.value.some(f => f.name === entry.name)) {
            checkedFiles.value.push({ ...entry });
        }
    } else {
        checkedFiles.value = checkedFiles.value.filter(f => f.name !== entry.name);
    }
}

async function uploadFile() {
    if (!isConnected.value) {
        uploadMessage.value = '请先连接服务器';
        return;
    }
    if (checkedFiles.value.length === 0) {
        uploadMessage.value = '请先勾选文件';
        return;
    }
    const total = checkedFiles.value.length;
    uploadMessage.value = `正在上传 ${total} 个文件...`;
    let success = 0, failed = 0;
    const tasks = [...checkedFiles.value];
    checkedFiles.value = [];
    for (const file of tasks) {
        const localPath = currentLocalPath.value === '/' ? file.name : `${currentLocalPath.value}/${file.name}`;
        try {
            await invoke('upload_local_file', { localPath, targetDir: currentRemotePath.value });
            success++;
        } catch (e) {
            failed++;
        }
    }
    uploadMessage.value = `上传完成：成功 ${success}，失败 ${failed}`;
    await Promise.all([listRemoteDir(currentRemotePath.value), listLocalDir(currentLocalPath.value)]);
}

onMounted(() => {
    listLocalDir('/');
    nextTick(drawCharts);
});
</script>

<template>
    <div id="file-client-container">
        
        <div data-tauri-drag-region class="custom-titlebar">
            <div class="title-text">Rust gRPC 文件传输客户端</div>
            <div class="window-controls">
                <button @click="minimize" class="control-btn minimize"><i class="fas fa-window-minimize"></i></button>
                <button @click="maximizeOrUnmaximize" class="control-btn maximize"><i class="fas fa-window-maximize"></i></button>
                <button @click="closeApp" class="control-btn close"><i class="fas fa-times"></i></button>
            </div>
        </div>
        
        <header>
            <div class="connection-bar">
                <input v-model="serverUrl" placeholder="服务器地址 (推荐 http://127.0.0.1:50051)" />
                <button @click="connectServer" :disabled="isConnected" class="btn connect-btn">
                    {{ isConnected ? '已连接' : '连接' }}
                </button>
                <span :class="['status-badge', { connected: isConnected, error: connectionStatus.includes('失败') }]">
                    状态: {{ connectionStatus }}
                </span>
            </div>
            <p class="upload-status">{{ uploadMessage }}</p>
        </header>

        <main class="file-transfer-main">
            <section class="local-panel panel list-area">
                <div class="panel-header">
                    <h2>本机目录: {{ currentLocalPath || '/' }}</h2>
                    <button @click="listLocalDir(currentLocalPath)" class="btn refresh-btn">
                        <i class="fas fa-sync-alt"></i>
                    </button>
                </div>
                <div class="file-list-container">
                    <table class="file-table local-table">
                        <thead>
                            <tr>
                                <th class="col-check"></th>
                                <th class="col-icon"></th>
                                <th class="col-name">名称</th>
                                <th class="col-size">大小</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="entry in localFiles" :key="entry.name"
                                :class="{ 'dir-entry': entry.is_dir, 'file-entry': !entry.is_dir, 'parent-dir': entry.is_parent }"
                                @click="handleLocalClick(entry)">
                                <td class="col-check">
                                    <input v-if="!entry.is_dir && !entry.is_parent" type="checkbox"
                                        :checked="checkedFiles.some(f => f.name === entry.name)"
                                        @click.stop="handleFileCheck(entry, $event)" />
                                </td>
                                <td class="col-icon"><i :class="getIconClass(entry)"></i></td>
                                <td class="col-name" :title="entry.name">{{ entry.name }}</td>
                                <td class="col-size">{{ entry.is_dir || entry.is_parent ? '-' : formatBytes(entry.size) }}</td>
                            </tr>
                            <tr v-if="localFiles.length === 0">
                                <td colspan="4" class="empty-item">目录为空</td>
                            </tr>
                        </tbody>
                    </table>
                </div>
            </section>

            <div class="action-center">
                <button @click="uploadFile" :disabled="!isConnected || checkedFiles.length === 0" class="btn upload-btn">
                    <i class="fas fa-cloud-upload-alt"></i>
                    上传 {{ checkedFiles.length }} 个文件<br>→ {{ currentRemotePath || '/' }}
                </button>
            </div>

            <section class="remote-panel panel list-area">
                <div class="panel-header">
                    <h2>远程目录: {{ currentRemotePath }}</h2>
                    <button @click="listRemoteDir(currentRemotePath)" :disabled="!isConnected" class="btn refresh-btn">
                        <i class="fas fa-sync-alt"></i>
                    </button>
                </div>
                <div class="file-list-container">
                    <table class="file-table remote-table">
                        <thead>
                            <tr>
                                <th class="col-icon"></th>
                                <th class="col-name">名称</th>
                                <th class="col-size remote-type-col">类型</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="entry in remoteFiles" :key="entry.name"
                                :class="{ 'dir-entry': entry.is_dir, 'file-entry': !entry.is_dir, 'parent-dir': entry.is_parent }"
                                @click="handleRemoteClick(entry)">
                                <td class="col-icon"><i :class="getIconClass(entry)"></i></td>
                                <td class="col-name" :title="entry.name">{{ entry.name }}</td>
                                <td class="col-size remote-type-col">{{ entry.is_dir ? '目录' : '文件' }}</td>
                            </tr>
                            <tr v-if="remoteFiles.length === 0 && isConnected">
                                <td colspan="3" class="empty-item">目录为空</td>
                            </tr>
                            <tr v-if="!isConnected">
                                <td colspan="3" class="empty-item">请连接服务器</td>
                            </tr>
                        </tbody>
                    </table>
                </div>
            </section>

            <section class="local-chart-panel panel chart-area">
                <div class="chart-grid">
                    <div ref="localTypeChartRef" class="chart-item"></div>
                    <div ref="localSizeChartRef" class="chart-item"></div>
                </div>
            </section>

            <section class="info-area panel chart-area">
                <div class="info-text">
                    <p>选中文件：{{ checkedFiles.length }} 个</p>
                    <p v-if="checkedFiles.length > 0">
                        总大小：{{ formatBytes(checkedFiles.reduce((sum, f) => sum + f.size, 0)) }}
                    </p>
                </div>
            </section>

            <section class="remote-chart-panel panel chart-area">
                <div ref="remoteTypeChartRef" class="chart-item full"></div>
            </section>
        </main>
    </div>
</template>

<style scoped>
/* ----------------------------------------------------------------
   全局容器样式
   ---------------------------------------------------------------- */
#file-client-container {
    font-family: system-ui, -apple-system, sans-serif;
    padding: 20px;
    background: linear-gradient(135deg, #f0f4f8 0%, #d9e2ec 100%);
    height: 100vh; 
    overflow: hidden; 
    
    display: flex;
    flex-direction: column;
}

/* ----------------------------------------------------------------
   🚀 新增/修改：自定义标题栏样式
   ---------------------------------------------------------------- */
.custom-titlebar {
    height: 35px; /* 标题栏高度 */
    background-color: #3b82f6; /* 蓝色背景 */
    color: white;
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0 10px;
    user-select: none; /* 防止拖拽时选中文字 */
    flex-shrink: 0;
    /* 圆角与 header 衔接 */
    border-top-left-radius: 12px;
    border-top-right-radius: 12px;
    -webkit-app-region: drag; /* 核心：启用拖拽 */
}

.title-text {
    font-size: 1rem;
    font-weight: 600;
    margin-left: 8px;
}

.window-controls {
    display: flex;
    gap: 0;
    -webkit-app-region: no-drag; /* 核心：禁用控制按钮的拖拽 */
}

.control-btn {
    width: 35px;
    height: 35px;
    background: transparent;
    border: none;
    color: white;
    font-size: 0.8rem;
    transition: background-color 0.2s;
    display: flex;
    justify-content: center;
    align-items: center;
    cursor: pointer;
    padding: 0;
}

.control-btn:hover {
    background-color: rgba(255, 255, 255, 0.2);
}

.control-btn.close:hover {
    background-color: #ef4444; /* 关闭按钮使用红色 */
    color: white;
}
/* ---------------------------------------------------------------- */

header {
    background: rgba(255, 255, 255, 0.9);
    backdrop-filter: blur(10px);
    padding: 16px;
    /* 移除顶部圆角，使其与自定义标题栏连接 */
    border-radius: 0 0 12px 12px;
    box-shadow: 0 4px 15px rgba(0,0,0,0.08);
    margin-bottom: 20px;
    margin-top: 0; /* 紧贴标题栏 */
    flex-shrink: 0; 
}

/* 隐藏原有的 h1，或将其大小减小 */
.h1-small {
    display: none; /* 在自定义标题栏中显示，所以这里隐藏 */
}

.connection-bar {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
}

.connection-bar input {
    flex: 1;
    min-width: 260px;
    padding: 10px 14px;
    border: 1px solid #e2e8f0;
    border-radius: 10px;
    font-size: 0.95rem;
}

.connect-btn {
    padding: 10px 24px;
    background: #3b82f6;
    color: white;
    border-radius: 10px;
    font-weight: 600;
}

.status-badge {
    padding: 8px 16px;
    border-radius: 10px;
    font-weight: 600;
    font-size: 0.95rem;
}

.status-badge.connected { background: #dcfce7; color: #166534; }
.status-badge.error { background: #fee2e2; color: #991b1b; }

.upload-status {
    margin-top: 12px;
    font-size: 1rem;
    color: #475569;
}

.file-transfer-main {
    display: grid;
    grid-template-columns: 1fr 180px 1fr;
    grid-template-rows: 3fr 1fr;
    gap: 20px;
    flex: 1; 
    min-height: 0; 
}

.panel {
    background: rgba(255, 255, 255, 0.95);
    backdrop-filter: blur(10px);
    border-radius: 12px;
    box-shadow: 0 4px 15px rgba(0,0,0,0.08);
    display: flex;
    flex-direction: column;
    overflow: hidden; 
}

.list-area { grid-row: 1 / 2; }
.chart-area { grid-row: 2 / 3; }

.panel-header {
    padding: 12px 16px;
    background: #f8fafc;
    border-bottom: 1px solid #e2e8f0;
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-shrink: 0; 
}

.panel-header h2 {
    font-size: 1.1rem;
    margin: 0;
    color: #1e293b;
}

.refresh-btn {
    padding: 6px 10px;
    background: #3b82f6;
    color: white;
    border-radius: 8px;
}

/* ----------------------------------------------------------------
   文件列表容器滚动
   ---------------------------------------------------------------- */
.file-list-container {
    flex: 1; 
    overflow-y: auto; 
    overflow-x: hidden;
    background: #fafafa;
}
/* ---------------------------------------------------------------- */

.file-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
    table-layout: fixed;
}

.file-table thead {
    position: sticky;
    top: 0;
    background: #e2e8f0;
    z-index: 10;
}

.file-table th, .file-table td {
    padding: 8px 12px; 
    text-align: left;
    border-bottom: 1px solid #f0f0f0;
    overflow: hidden; 
    white-space: nowrap; 
}

/* --- 本地表格列宽定义 (4列) --- */
.local-table .col-check { width: 40px; text-align: center; }
.local-table .col-icon { width: 40px; text-align: center; }
.local-table .col-size { width: 120px; text-align: right; }
.local-table .col-name { width: calc(100% - 200px); } 

/* --- 远程表格列宽定义 (3列) --- */
.remote-table .col-icon { width: 40px; text-align: center; }
.remote-table .remote-type-col { width: 100px; text-align: center; } 
.remote-table .col-name { width: calc(100% - 140px); } 
.remote-table .col-size { text-align: center; } 


.file-table tr:hover { background: #f0f9ff; }
.col-check input { transform: scale(1.2); }
.col-icon { color: #64748b; font-size: 1em; }
.col-name { color: #1e293b; }

.col-name:hover {
    overflow: visible;
    white-space: normal;
    background: white;
    z-index: 1;
    box-shadow: 0 2px 6px rgba(0,0,0,0.1);
    padding: 4px;
    border-radius: 4px;
}

.dir-entry .col-name { color: #3b82f6; font-weight: 500; }
.parent-dir .col-name { color: #94a3b8; font-style: italic; }
.col-size { color: #64748b; }

.empty-item {
    text-align: center;
    padding: 40px;
    color: #94a3b8;
}

.action-center {
    grid-row: 1 / 2;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 20px;
}

.upload-btn {
    background: linear-gradient(135deg, #10b981, #34d399);
    color: white;
    padding: 14px 24px;
    border-radius: 16px;
    font-size: 1rem;
    box-shadow: 0 6px 20px rgba(16,185,129,0.3);
}

.upload-btn i { font-size: 1.6rem; margin-bottom: 6px; }

/* 图表区 */
.chart-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    width: 100%;
    height: 100%;
    padding: 12px;
    box-sizing: border-box;
}

.chart-item {
    background: #fff;
    border-radius: 8px;
    box-shadow: 0 2px 8px rgba(0,0,0,0.05);
}

.chart-item.full {
    grid-column: 1 / -1;
}

.info-area {
    display: flex;
    align-items: center;
    justify-content: center;
}

.info-text {
    text-align: center;
    color: #475569;
    font-size: 1rem;
}
</style>