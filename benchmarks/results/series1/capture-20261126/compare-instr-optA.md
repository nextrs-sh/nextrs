| Requests | n | same decoded body | same status | same encoding | wire bytes instr | wire bytes optA |
|---|---:|---:|---:|---:|---:|---:|
| static gzip | 19 | 19 | 19 | 19 | 332770 | 332770 |
| static identity | 19 | 19 | 19 | 19 | 1063899 | 1063899 |
| static HEAD gzip | 1 | 1 | 1 | 1 | 0 | 0 |
| static If-None-Match | 1 | 1 | 1 | 1 | 0 | 0 |
| static range | 1 | 1 | 1 | 1 | 100 | 100 |
| static range gzip | 1 | 1 | 1 | 1 | 100 | 100 |
| rsc gzip | 14 | 14 | 14 | 14 | 154274 | 154274 |
| rsc identity | 14 | 14 | 14 | 14 | 714774 | 714774 |
| rsc without _rsc | 1 | 1 | 1 | 1 | 0 | 0 |
| html gzip | 3 | 3 | 3 | 3 | 73105 | 73105 |
| html identity | 3 | 3 | 3 | 3 | 393689 | 393689 |

0 difference(s).
