# Agent Playbook

Use `skills/qql-skill/SKILL.md` for QQL syntax and keep the investigation flow narrow.

## Command order

```bash
qql-go doctor --quiet --json
qql-go exec --quiet --json "SHOW COLLECTION medical_retrieval_ops"
qql-go explain --quiet --json "QUERY '<medical question>' FROM medical_retrieval_ops LIMIT 5 USING HYBRID"
qql-go exec --quiet --json "QUERY '<medical question>' FROM medical_retrieval_ops LIMIT 5 USING HYBRID"
qql-go exec --quiet --json "QUERY '<medical question>' FROM medical_retrieval_ops LIMIT 5 SCORE THRESHOLD 0.6 USING HYBRID"
qql-go exec --quiet --json "QUERY '<medical question>' FROM medical_retrieval_ops LIMIT 5 OFFSET 5 USING HYBRID"
qql-go exec --quiet --json "QUERY '<medical question>' FROM medical_retrieval_ops LIMIT 5 USING SPARSE"
qql-go exec --quiet --json "QUERY '<medical question>' FROM medical_retrieval_ops LIMIT 5 USING HYBRID WHERE specialty = '<expected specialty>'"
qql-go exec --quiet --json "QUERY '<medical question>' FROM medical_retrieval_ops LIMIT 6 SCORE THRESHOLD 0.5 USING HYBRID GROUP BY specialty GROUP_SIZE 2"
qql-go exec --quiet --json "SELECT * FROM medical_retrieval_ops WHERE id = <best_result_id>"
qql-go exec --quiet --json "QUERY RECOMMEND WITH (positive = (<best_result_id>)) FROM medical_retrieval_ops LIMIT 5"
qql-go exec --quiet --json "QUERY ORDER BY case_priority DESC FROM medical_retrieval_ops LIMIT 5"
qql-go exec --quiet --json "QUERY '<medical question>' FROM medical_retrieval_ops LIMIT 5 USING HYBRID WITH PAYLOAD (exclude = ['reference'])"
```

## Report

Return:

- short answer
- best result ID and specialty
- whether hybrid and sparse agreed on the top result
- one supporting related record
- brief medical-information-only limitation note
