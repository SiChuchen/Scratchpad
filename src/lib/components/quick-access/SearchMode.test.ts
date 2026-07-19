import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { allKindSummaries, fixtureDetail } from '../../../test/fixtures/content'

const m=vi.hoisted(()=>({searchLocal:vi.fn(),planSearch:vi.fn(),cancelPlan:vi.fn(),detail:vi.fn(),openInMain:vi.fn(),copyText:vi.fn(),copyImage:vi.fn(),copyFile:vi.fn()}))
vi.mock('$lib/api/content',()=>({contentApi:{searchLocal:m.searchLocal,planSearch:m.planSearch,cancelPlan:m.cancelPlan,detail:m.detail,openInMain:m.openInMain}}))
vi.mock('$lib/api/vault',()=>({vaultApi:{copyText:m.copyText}}))
vi.mock('$lib/api/dock',()=>({dockApi:{copyImage:m.copyImage,copyFile:m.copyFile}}))
import SearchMode from './SearchMode.svelte'

const hit=(summary:any)=>({summary,score:1,sources:['local']})
beforeEach(()=>{vi.useFakeTimers();for(const x of Object.values(m))x.mockReset();m.cancelPlan.mockResolvedValue(undefined);m.openInMain.mockResolvedValue(undefined);m.copyText.mockResolvedValue(undefined);m.copyImage.mockResolvedValue(undefined);m.copyFile.mockResolvedValue(undefined)})
afterEach(()=>{cleanup();vi.useRealTimers()})

describe('unified quick search',()=>{
  it('shows temporary and saved results without storage source labels',async()=>{
    const text=allKindSummaries.find(x=>x.kind==='text')!;const credential=allKindSummaries.find(x=>x.kind==='credential')!
    m.searchLocal.mockResolvedValue([hit(text),hit(credential)]);m.detail.mockImplementation(async(id:string)=>fixtureDetail(allKindSummaries.find(x=>x.id===id)!))
    render(SearchMode,{notify:vi.fn(),autoHybridSearch:false});await fireEvent.input(screen.getByRole('searchbox'),{target:{value:'生产'}});await vi.advanceTimersByTimeAsync(300);await Promise.resolve()
    expect(screen.getAllByText(text.title).length).toBeGreaterThan(0);expect(screen.getByText(credential.title)).toBeVisible();expect(screen.getAllByText('临时').length).toBeGreaterThan(0);expect(screen.getAllByText('已收藏').length).toBeGreaterThan(0)
    expect(screen.queryByText(/^Local$/)).toBeNull();expect(screen.queryByText(/^AI$/)).toBeNull();expect(screen.queryByText(/^Vault$/)).toBeNull()
  })

  it('keeps selection across refresh and exposes unified detail actions',async()=>{
    const credential=allKindSummaries.find(x=>x.kind==='credential')!;m.searchLocal.mockResolvedValue([hit(credential)]);m.detail.mockResolvedValue(fixtureDetail(credential))
    const view=render(SearchMode,{notify:vi.fn(),autoHybridSearch:false,refreshToken:0});await fireEvent.input(screen.getByRole('searchbox'),{target:{value:'账号'}});await vi.advanceTimersByTimeAsync(300)
    expect(await screen.findByRole('button',{name:'复制密码'})).toBeVisible();await view.rerender({notify:vi.fn(),autoHybridSearch:false,refreshToken:1});await vi.advanceTimersByTimeAsync(0);await vi.advanceTimersByTimeAsync(300)
    expect(m.searchLocal).toHaveBeenCalledTimes(2);expect(screen.getByRole('option')).toHaveAttribute('aria-selected','true')
  })

  it('ignores late results from an older query', async () => {
    let resolveOld!: (value: unknown[]) => void
    const old = new Promise<unknown[]>((resolve) => { resolveOld = resolve })
    const text = allKindSummaries.find((item) => item.kind === 'text')!
    const note = allKindSummaries.find((item) => item.kind === 'note')!
    m.searchLocal.mockImplementation((query: string) => query === 'old' ? old : Promise.resolve([hit(note)]))
    m.detail.mockImplementation(async (id: string) => fixtureDetail(allKindSummaries.find((item) => item.id === id)!))
    render(SearchMode, { notify: vi.fn(), autoHybridSearch: false })
    const input = screen.getByRole('searchbox')
    await fireEvent.input(input, { target: { value: 'old' } })
    await vi.advanceTimersByTimeAsync(300)
    await fireEvent.input(input, { target: { value: 'new' } })
    await vi.advanceTimersByTimeAsync(300)
    resolveOld([hit(text)])
    await Promise.resolve()
    expect(screen.getAllByText(note.title).length).toBeGreaterThan(0)
    expect(screen.queryAllByText(text.title)).toHaveLength(0)
  })

  it('moves selection with the keyboard and loads the selected detail', async () => {
    const first = allKindSummaries[0]!
    const second = allKindSummaries[1]!
    m.searchLocal.mockResolvedValue([hit(first), hit(second)])
    m.detail.mockImplementation(async (id: string) => fixtureDetail(allKindSummaries.find((item) => item.id === id)!))
    render(SearchMode, { notify: vi.fn(), autoHybridSearch: false })
    const input = screen.getByRole('searchbox')
    await fireEvent.input(input, { target: { value: 'item' } })
    await vi.advanceTimersByTimeAsync(300)
    await fireEvent.keyDown(input, { key: 'ArrowDown' })
    await Promise.resolve()
    expect(screen.getAllByRole('option')[1]).toHaveAttribute('aria-selected', 'true')
    expect(m.detail).toHaveBeenLastCalledWith(second.id)
  })

  it('keeps local results when optional AI planning fails', async () => {
    const text = allKindSummaries.find((item) => item.kind === 'text')!
    m.searchLocal.mockResolvedValue([hit(text)])
    m.detail.mockResolvedValue(fixtureDetail(text))
    m.planSearch.mockRejectedValue(new Error('offline'))
    render(SearchMode, { notify: vi.fn(), autoHybridSearch: true })
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'offline' } })
    await vi.advanceTimersByTimeAsync(1000)
    await Promise.resolve()
    expect(screen.getAllByText(text.title).length).toBeGreaterThan(0)
    expect(m.planSearch).toHaveBeenCalledOnce()
  })

  it.each(allKindSummaries)('renders and executes the primary action for $kind', async (summary) => {
    m.searchLocal.mockResolvedValue([hit(summary)])
    m.detail.mockResolvedValue(fixtureDetail(summary))
    render(SearchMode, { notify: vi.fn(), autoHybridSearch: false })
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: summary.kind } })
    await vi.advanceTimersByTimeAsync(300)
    await Promise.resolve()
    const label = summary.kind === 'text' ? '复制文本'
      : summary.kind === 'image' ? '复制图片'
      : summary.kind === 'file' ? '复制文件'
      : summary.kind === 'credential' ? '复制密码'
      : summary.kind === 'bookmark' ? '打开链接'
      : '复制笔记'
    await fireEvent.click(screen.getByRole('button', { name: label }))
    if (summary.kind === 'text' || summary.kind === 'note') expect(m.copyText).toHaveBeenCalled()
    if (summary.kind === 'credential') expect(m.copyText).toHaveBeenCalledWith(expect.any(String), true)
    if (summary.kind === 'image') expect(m.copyImage).toHaveBeenCalled()
    if (summary.kind === 'file') expect(m.copyFile).toHaveBeenCalled()
  })
})
