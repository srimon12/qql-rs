$ErrorActionPreference = "Stop"

if (-not (Get-Command qql-go -ErrorAction SilentlyContinue)) {
    throw "qql-go must be installed and available on PATH"
}
if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
    throw "medical-retrieval-ops requires uv"
}

$demoRoot = $PSScriptRoot
$artifacts = if ($env:MEDICAL_RAG_ARTIFACTS) { $env:MEDICAL_RAG_ARTIFACTS } else { Join-Path $demoRoot "artifacts" }
$generated = if ($env:MEDICAL_RAG_GENERATED_DIR) { $env:MEDICAL_RAG_GENERATED_DIR } else { Join-Path $demoRoot "generated" }
$collection = "medical_retrieval_ops"

if (Test-Path $artifacts) {
    Remove-Item -Recurse -Force $artifacts
}
if (Test-Path $generated) {
    Remove-Item -Recurse -Force $generated
}
New-Item -ItemType Directory -Path $artifacts | Out-Null
New-Item -ItemType Directory -Path $generated | Out-Null

function Run-Step {
    param(
        [string]$Id,
        [string]$Command,
        [string]$Statement
    )

    $artifact = Join-Path $artifacts "$Id.json"
    $raw = & qql-go $Command --quiet --json $Statement
    $raw | Set-Content -Path $artifact -Encoding utf8
    $json = $raw | ConvertFrom-Json
    if (-not $json.ok) {
        throw "Step '$Id' failed: $raw"
    }
}

Write-Host "Building full medical benchmark corpus..."
$env:MEDICAL_RAG_GENERATED_DIR = $generated
if (-not $env:MEDICAL_RAG_MAX_ROWS) {
    $env:MEDICAL_RAG_MAX_ROWS = "all"
}
(& uv run (Join-Path $demoRoot "build-medical-corpus.py")) | Set-Content -Path (Join-Path $artifacts "00-build.json") -Encoding utf8

$eval = Get-Content -Raw -Path (Join-Path $generated "eval.json") | ConvertFrom-Json
$mainQuery = $eval.queries.main.question.Replace("'", "\\'")
$mainId = $eval.queries.main.id
$mainSpecialty = $eval.queries.main.specialty.Replace("'", "\\'")
$mainTenant = $eval.queries.main.tenant_id.Replace("'", "\\'")
$mainPriority = $eval.queries.main.case_priority.Replace("'", "\\'")
$mainStatus = $eval.queries.main.case_status.Replace("'", "\\'")
$relatedId = $eval.queries.related.id

Write-Host "Running medical retrieval operations..."
(& qql-go doctor --quiet --json) | Set-Content -Path (Join-Path $artifacts "01-doctor.json") -Encoding utf8
& qql-go exec --quiet --json "DROP COLLECTION $collection" | Out-Null
(& qql-go execute --quiet --json (Join-Path $demoRoot "01-provision.qql")) | Set-Content -Path (Join-Path $artifacts "02-provision.json") -Encoding utf8
(& qql-go execute --quiet --json (Join-Path $generated "02-seed.qql")) | Set-Content -Path (Join-Path $artifacts "03-seed.json") -Encoding utf8

Run-Step "04-inspect" "exec" "SHOW COLLECTION $collection"
Run-Step "05-explain-hybrid-rrf" "explain" "QUERY '$mainQuery' FROM $collection LIMIT 5 USING HYBRID"
Run-Step "06-search-dense" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5"
Run-Step "07-search-sparse" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5 USING SPARSE"
Run-Step "08-search-hybrid-rrf" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5 USING HYBRID"
Run-Step "09-search-hybrid-dbsf" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5 USING HYBRID FUSION DBSF"
Run-Step "10-search-exact" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5 EXACT"
Run-Step "11-search-filtered-tenant" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5 WHERE tenant_id = '$mainTenant' AND case_status = '$mainStatus' AND case_priority = '$mainPriority' WITH (acorn = true)"
Run-Step "12-search-score-threshold" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5 SCORE THRESHOLD 0.0 USING HYBRID"
Run-Step "13-search-offset-window" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5 OFFSET 1 USING HYBRID"
Run-Step "14-search-grouped-specialty" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 6 SCORE THRESHOLD 0.0 USING HYBRID GROUP BY 'specialty' GROUP_SIZE 2"
Run-Step "15-search-hybrid-mmr" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 5 USING HYBRID WITH (mmr_diversity = 0.5, mmr_candidates = 20)"
Run-Step "15b-search-prefetch-rrf" "exec" "WITH a AS (QUERY '$mainQuery' USING dense LIMIT 20), b AS (QUERY '$mainQuery' USING sparse LIMIT 20) QUERY '$mainQuery' FROM $collection LIMIT 5 PREFETCH (a, b) FUSION RRF"
Run-Step "15c-search-prefetch-rrf-per-filter" "exec" "WITH a AS (QUERY '$mainQuery' USING dense LIMIT 20), b AS (QUERY '$mainQuery' USING sparse LIMIT 20) QUERY '$mainQuery' FROM $collection LIMIT 5 PREFETCH (a WHERE case_priority = '$mainPriority' SCORE THRESHOLD 0.5, b SCORE THRESHOLD 0.3) FUSION RRF WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])"
Run-Step "15d-search-grouped-with-lookup" "exec" "QUERY '$mainQuery' FROM $collection LIMIT 6 GROUP BY 'specialty' GROUP_SIZE 2"
Run-Step "16-select-main" "exec" "SELECT * FROM $collection WHERE id = $mainId"
Run-Step "17-recommend-related" "exec" "QUERY RECOMMEND WITH (positive = ($relatedId)) FROM $collection LIMIT 5"
Run-Step "18-scroll-tenant" "exec" "SCROLL FROM $collection WHERE tenant_id = '$mainTenant' LIMIT 5"
(& qql-go dump --quiet --json $collection (Join-Path $artifacts "backup.qql")) | Set-Content -Path (Join-Path $artifacts "19-dump.json") -Encoding utf8
(& uv run (Join-Path $demoRoot "run-benchmark.py") (Join-Path $generated "benchmark-questions.json")) | Set-Content -Path (Join-Path $artifacts "20-benchmark.json") -Encoding utf8

& (Join-Path $demoRoot "validate-artifacts.ps1") -EvalPath (Join-Path $generated "eval.json") -Artifacts $artifacts

Write-Host "Workflow complete. Artifacts saved to: $artifacts"
