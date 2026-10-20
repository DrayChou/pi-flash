# sync-issues.ps1 — 拉取 GitHub issues 镜像到 docs/issues.md
# 由 scripts/sync-issues.bat 调用；失败时保留旧文件不动。
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$repoRoot = Split-Path -Parent $PSScriptRoot
$outPath  = Join-Path $repoRoot 'docs\issues.md'
$tmpPath  = $outPath + '.tmp'

try {
    # ---- 仓库定位：从 git remote 解析（兼容 ssh 别名 / git@ / https） ----
    $remote = (& git -C $repoRoot remote get-url origin 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $remote) { throw "读取 git remote origin 失败" }
    $repo = $remote.Trim()
    if ($repo -match '^https?://') { $repo = $repo -replace '^https?://[^/]+/', '' }
    else                          { $repo = $repo -replace '^[^@]+@', '' -replace '^[^:]+:', '' }
    $repo = ($repo -replace '\.git$', '').Trim('/')
    if ($repo -notmatch '^[^/]+/[^/]+$') { throw "remote 解析异常: $remote" }

    # ---- 拉取 issue 列表 ----
    $raw = gh issue list -R $repo --state all --limit 200 `
        --json number,title,state,labels,createdAt,updatedAt,closedAt,author,url,body
    if ($LASTEXITCODE -ne 0) { throw "gh issue list 失败（exit $LASTEXITCODE）" }
    $issues = $raw | ConvertFrom-Json
    if (-not $issues) { $issues = @() }

    # ---- 逐条补评论（失败不致命） ----
    foreach ($i in $issues) {
        $i | Add-Member -NotePropertyName _comments -NotePropertyValue @() -Force
        $view = gh issue view $i.number -R $repo --json comments 2>$null
        if ($LASTEXITCODE -eq 0 -and $view) {
            $i._comments = @(($view | ConvertFrom-Json).comments)
        }
    }

    # ---- 排序：open 在前、closed 在后，各按编号倒序 ----
    $ordered = @($issues | Where-Object { $_.state -eq 'OPEN'  } | Sort-Object number -Descending) `
             + @($issues | Where-Object { $_.state -eq 'CLOSED' } | Sort-Object number -Descending)

    # ---- 渲染 markdown ----
    $syncAt = (Get-Date).ToString('yyyy-MM-dd HH:mm')
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# GitHub Issues 镜像')
    [void]$sb.AppendLine()
    [void]$sb.AppendLine("> 仓库：$repo ｜ 同步：$syncAt ｜ 共 $($issues.Count) 条（open $(@($issues | Where-Object state -eq 'OPEN').Count)）")
    [void]$sb.AppendLine('>')
    [void]$sb.AppendLine('> 本文件由 `scripts\sync-issues.bat` 自动生成，手动修改会被下次同步覆盖；评论/关闭请走 `gh issue`。')
    [void]$sb.AppendLine()

    foreach ($group in @(@('OPEN', '🔴 Open'), @('CLOSED', '✅ Closed'))) {
        $sel = @($ordered | Where-Object { $_.state -eq $group[0] })
        if ($sel.Count -eq 0) { continue }
        [void]$sb.AppendLine("## $($group[1])（$($sel.Count)）")
        [void]$sb.AppendLine()
        foreach ($i in $sel) {
            $labels = ($i.labels | ForEach-Object { $_.name }) -join ', '
            $author = $i.author.login
            $closed = if ($i.closedAt) { " ｜ 关闭：$($i.closedAt.Substring(0,10))" } else { '' }
            [void]$sb.AppendLine("### #$($i.number) $($i.title)")
            [void]$sb.AppendLine("- 状态：$($i.state.ToLower()) ｜ 标签：$(if ($labels) { $labels } else { '—' }) ｜ 作者：$author ｜ 创建：$($i.createdAt.Substring(0,10)) ｜ 更新：$($i.updatedAt.Substring(0,10))$closed")
            [void]$sb.AppendLine("- $($i.url)")
            [void]$sb.AppendLine()
            $body = "$($i.body)".Trim()
            if ($body) { [void]$sb.AppendLine($body); [void]$sb.AppendLine() }
            if ($i._comments.Count -gt 0) {
                [void]$sb.AppendLine("<details><summary>评论（$($i._comments.Count)）</summary>")
                [void]$sb.AppendLine()
                foreach ($c in $i._comments) {
                    [void]$sb.AppendLine("**@$($c.author.login)**（$($c.createdAt.Substring(0,10))）：")
                    [void]$sb.AppendLine()
                    $ctext = "$($c.body)".Trim()
                    [void]$sb.AppendLine($ctext)
                    [void]$sb.AppendLine()
                }
                [void]$sb.AppendLine('</details>')
                [void]$sb.AppendLine()
            }
        }
    }

    # ---- 原子落盘：先写 .tmp 再替换，失败保留旧文件 ----
    [System.IO.File]::WriteAllText($tmpPath, $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))
    Move-Item -Force $tmpPath $outPath
    Write-Host "[OK] $repo -> docs\issues.md（$($issues.Count) 条，同步于 $syncAt）" -ForegroundColor Green
    exit 0
}
catch {
    Write-Host "[FAIL] $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "       已保留旧的 docs\issues.md（如有）" -ForegroundColor Yellow
    if (Test-Path $tmpPath) { Remove-Item -Force $tmpPath -ErrorAction SilentlyContinue }
    exit 1
}
