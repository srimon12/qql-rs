param(
    [Parameter(Mandatory=$true)] [string]$EvalPath,
    [Parameter(Mandatory=$true)] [string]$Artifacts
)

$ErrorActionPreference = "Stop"

function Assert-True {
    param(
        [bool]$Condition,
        [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Read-Json {
    param([string]$Path)
    return Get-Content -Raw -Path $Path | ConvertFrom-Json
}

$eval = Read-Json $EvalPath
$mainId = [string]$eval.queries.main.id
$mainSpecialty = $eval.queries.main.specialty
$mainTenant = $eval.queries.main.tenant_id
$mainPriority = $eval.queries.main.case_priority
$mainStatus = $eval.queries.main.case_status
$minRows = $eval.row_count

$doctor = Read-Json (Join-Path $Artifacts "01-doctor.json")
Assert-True ($doctor.ok -and $doctor.healthy) "doctor should report a healthy connection"

$inspect = Read-Json (Join-Path $Artifacts "04-inspect.json")
Assert-True ($inspect.ok -and $inspect.data.topology -eq "hybrid" -and $inspect.data.points_count -ge $minRows -and $inspect.data.quantization -eq "scalar") "collection should be hybrid, quantized, and contain the generated rows"
Assert-True ($inspect.data.payload_schema.tenant_id.type -eq "keyword") "tenant payload index should remain keyword"
Assert-True ($inspect.data.payload_schema.topic_text.type -eq "text") "topic text payload index should remain text"
Assert-True ($inspect.data.payload_schema.case_priority.type -eq "keyword") "case priority payload index should remain keyword"

$explain = Read-Json (Join-Path $Artifacts "05-explain-hybrid-rrf.json")
Assert-True ($explain.ok -and $explain.plan.Contains("Using: HYBRID")) "hybrid explain plan should be present"

$dense = Read-Json (Join-Path $Artifacts "06-search-dense.json")
Assert-True ($dense.ok -and ($dense.data.id -contains $mainId)) "main document should appear in dense results"

$sparse = Read-Json (Join-Path $Artifacts "07-search-sparse.json")
Assert-True ($sparse.ok -and $sparse.data.Count -ge 1) "sparse search should return medical matches"

$hybridRrf = Read-Json (Join-Path $Artifacts "08-search-hybrid-rrf.json")
Assert-True ($hybridRrf.ok -and ($hybridRrf.data.id -contains $mainId)) "main document should appear in hybrid RRF results"

$hybridDbsf = Read-Json (Join-Path $Artifacts "09-search-hybrid-dbsf.json")
Assert-True ($hybridDbsf.ok -and ($hybridDbsf.data.id -contains $mainId)) "main document should appear in hybrid DBSF results"

$exact = Read-Json (Join-Path $Artifacts "10-search-exact.json")
Assert-True ($exact.ok -and ($exact.data.id -contains $mainId)) "main document should appear in exact results"

$filtered = Read-Json (Join-Path $Artifacts "11-search-filtered-tenant.json")
Assert-True ($filtered.ok -and $filtered.data.Count -ge 1 -and ($filtered.data.id -contains $mainId)) "tenant-filtered active high-priority search should keep the main document"

$threshold = Read-Json (Join-Path $Artifacts "12-search-score-threshold.json")
Assert-True ($threshold.ok -and $threshold.data.Count -ge 1) "score-thresholded hybrid search should return medical matches"

$offset = Read-Json (Join-Path $Artifacts "13-search-offset-window.json")
Assert-True ($offset.ok -and $offset.data.Count -ge 1) "offset hybrid search should return the next result window"

$grouped = Read-Json (Join-Path $Artifacts "14-search-grouped-specialty.json")
Assert-True ($grouped.ok -and $grouped.data.Count -ge 1) "grouped search should return groups"
Assert-True ($grouped.data.group_id -contains $mainSpecialty) "grouped search should surface the specialty groups"

$mmr = Read-Json (Join-Path $Artifacts "15-search-hybrid-mmr.json")
Assert-True ($mmr.ok -and $mmr.data.Count -ge 1) "hybrid MMR search should return diversified medical matches"

$select = Read-Json (Join-Path $Artifacts "16-select-main.json")
Assert-True $select.ok "selected document should exist by ID"
Assert-True ($select.data.payload.tenant_id -eq $mainTenant) "selected document should preserve tenant metadata"
Assert-True ($select.data.payload.case_priority -eq $mainPriority) "selected document should preserve priority metadata"
Assert-True ($select.data.payload.case_status -eq $mainStatus) "selected document should preserve status metadata"

$recommend = Read-Json (Join-Path $Artifacts "17-recommend-related.json")
Assert-True ($recommend.ok -and $recommend.data.Count -ge 1) "recommend should return related medical answers"

$scroll = Read-Json (Join-Path $Artifacts "18-scroll-tenant.json")
Assert-True ($scroll.ok -and $scroll.data.points.Count -ge 1) "tenant scroll should return related entries"
Assert-True (-not ($scroll.data.points | Where-Object { $_.payload.tenant_id -ne $mainTenant })) "tenant scroll should stay inside one tenant"

$dump = Read-Json (Join-Path $Artifacts "19-dump.json")
Assert-True $dump.ok "dump should succeed"

$benchmark = Read-Json (Join-Path $Artifacts "20-benchmark.json")
Assert-True ($benchmark.modes.Count -ge 5) "benchmark should report all retrieval modes"

Write-Host "Validated medical retrieval artifacts for question '$($eval.queries.main.question)' in $Artifacts" -ForegroundColor Green
