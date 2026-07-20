import { fireEvent, render } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'
import QuickContentDetail from './QuickContentDetail.svelte'
import { fixtureDetail, allKindSummaries } from '../../../test/fixtures/content'

const actions = () => ({ onCopyText:vi.fn(async()=>{}), onCopyFile:vi.fn(async()=>{}), onOpen:vi.fn(async()=>{}), onManage:vi.fn(async()=>{}), onNotify:vi.fn() })

describe('QuickContentDetail', () => {
  it.each([
    ['text','复制文本'],['image','复制图片'],['file','复制文件'],['credential','复制密码'],['bookmark','打开链接'],['note','复制笔记'],
  ] as const)('offers the fastest useful action for %s', (kind, label) => {
    const detail=fixtureDetail(allKindSummaries.find(x=>x.kind===kind)!)
    const view=render(QuickContentDetail,{props:{detail,resetToken:0,...actions()}})
    expect(view.getByRole('button',{name:label})).toBeVisible()
  })

  it('orders useful credential fields before notes and keeps copy last', () => {
    const detail=fixtureDetail(allKindSummaries.find(x=>x.kind==='credential')!) as any
    detail.fields=[{key:'密码',value:'dummy-password',isSensitive:true,sortOrder:1},{key:'账号',value:'alice',isSensitive:false,sortOrder:0}]
    detail.notes='稍后轮换'
    const view=render(QuickContentDetail,{props:{detail,resetToken:0,...actions()}})
    const rows=[...view.container.querySelectorAll('[data-field-row]')]
    expect(rows).toHaveLength(2)
    for(const row of rows){expect(row.lastElementChild).toHaveAttribute('data-copy-action');expect(row.lastElementChild).toHaveClass('quick-copy-target')}
    expect(view.container.textContent!.indexOf('账号')).toBeLessThan(view.container.textContent!.indexOf('稍后轮换'))
  })

  it('copies sensitive values without revealing and re-masks on reset', async () => {
    const detail=fixtureDetail(allKindSummaries.find(x=>x.kind==='credential')!) as any
    detail.fields=[{key:'密码',value:'dummy-password',isSensitive:true,sortOrder:0}]
    const a=actions();const view=render(QuickContentDetail,{props:{detail,resetToken:0,...a}})
    await fireEvent.click(view.getByRole('button',{name:'复制密码'}))
    expect(a.onCopyText).toHaveBeenCalledWith('dummy-password',true)
    expect(view.queryByText('dummy-password')).toBeNull()
  })

  it('copies the asset path for image and file entries', async () => {
    const writeText=vi.fn().mockResolvedValue(undefined)
    Object.assign(navigator,{clipboard:{writeText}})
    for(const kind of ['image','file'] as const){
      const detail=fixtureDetail(allKindSummaries.find(x=>x.kind===kind)!)
      const a=actions();const view=render(QuickContentDetail,{props:{detail,resetToken:0,...a}})
      await fireEvent.click(view.getByRole('button',{name:'复制路径'}))
      expect(writeText).toHaveBeenCalledWith(kind==='image'?'fixture/image.png':'fixture/file.pdf')
      expect(a.onNotify).toHaveBeenCalledWith('已复制路径','success')
      view.unmount()
    }
    const text=render(QuickContentDetail,{props:{detail:fixtureDetail(allKindSummaries.find(x=>x.kind==='text')!),resetToken:0,...actions()}})
    expect(text.queryByRole('button',{name:'复制路径'})).toBeNull()
  })
})
