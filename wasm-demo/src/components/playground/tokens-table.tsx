import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import type { QqlToken } from "@/lib/qql-types"
import { Badge } from "@/components/ui/badge"

type TokensTableProps = {
  tokens: QqlToken[]
}

export function TokensTable({ tokens }: TokensTableProps) {
  if (!tokens.length) {
    return (
      <p className="p-4 text-sm text-muted-foreground">No tokens generated.</p>
    )
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="w-[140px]">Kind</TableHead>
          <TableHead>Literal</TableHead>
          <TableHead className="w-[72px]">Start</TableHead>
          <TableHead className="w-[72px]">End</TableHead>
          <TableHead className="w-[56px]">Len</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {tokens.map((t, i) => (
          <TableRow key={`${t.pos}-${t.end}-${i}`}>
            <TableCell>
              <Badge variant="secondary" className="font-mono text-[10px]">
                {t.kind}
              </Badge>
            </TableCell>
            <TableCell className="max-w-[240px] truncate font-mono text-xs">
              {t.text}
            </TableCell>
            <TableCell className="font-mono text-xs text-muted-foreground">
              {t.pos}
            </TableCell>
            <TableCell className="font-mono text-xs text-muted-foreground">
              {t.end}
            </TableCell>
            <TableCell className="font-mono text-xs text-muted-foreground">
              {t.len}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}
