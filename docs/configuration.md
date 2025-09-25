# Environment Variable Configuration

The Compatibility Engine uses environment variables to configure calculation parameters. The constants for penalty rates, tax brackets, and other configuration values are completely removed from the API requests and are instead configured entirely through environment variables.

## Penalty Calculation Defaults

| Environment Variable | Description | Default Value | Example |
|---------------------|-------------|---------------|---------|
| `ENGINE_DEFAULT_RATE_PER_DAY` | Daily penalty rate | 100.0 | `ENGINE_DEFAULT_RATE_PER_DAY=150.0` |
| `ENGINE_DEFAULT_CAP` | Maximum penalty cap | 1000.0 | `ENGINE_DEFAULT_CAP=2000.0` |
| `ENGINE_DEFAULT_INTEREST_RATE` | Interest rate (decimal) | 0.05 | `ENGINE_DEFAULT_INTEREST_RATE=0.06` |

## Tax Calculation Defaults

| Environment Variable | Description | Default Value | Example |
|---------------------|-------------|---------------|---------|
| `ENGINE_DEFAULT_THRESHOLDS` | Tax bracket thresholds (comma-separated) | 10000.0 | `ENGINE_DEFAULT_THRESHOLDS=15000.0,50000.0,120000.0` |
| `ENGINE_DEFAULT_RATES` | Tax rates for each bracket (comma-separated) | 0.10,0.20 | `ENGINE_DEFAULT_RATES=0.12,0.25,0.35,0.40` |
| `ENGINE_DEFAULT_SURCHARGE_THRESHOLD` | Surcharge threshold | 5000.0 | `ENGINE_DEFAULT_SURCHARGE_THRESHOLD=7500.0` |
| `ENGINE_DEFAULT_SURCHARGE_RATE` | Surcharge rate (decimal) | 0.02 | `ENGINE_DEFAULT_SURCHARGE_RATE=0.025` |

## Usage

### With .env file

Create a `.env` file in the project root:

```
# Override defaults from LyFin-Compliance-Annex.md
ENGINE_DEFAULT_RATE_PER_DAY=150.0
ENGINE_DEFAULT_CAP=2000.0
ENGINE_DEFAULT_INTEREST_RATE=0.06

# Override defaults from 2025_61-FR.md
ENGINE_DEFAULT_THRESHOLDS=15000.0,50000.0,120000.0
ENGINE_DEFAULT_RATES=0.12,0.25,0.35,0.40
ENGINE_DEFAULT_SURCHARGE_THRESHOLD=7500.0
ENGINE_DEFAULT_SURCHARGE_RATE=0.025
```

### With system environment variables

```bash
export ENGINE_DEFAULT_RATE_PER_DAY=150.0
export ENGINE_DEFAULT_CAP=2000.0
# ... etc
```

### With Docker

```bash
docker run -e ENGINE_DEFAULT_RATE_PER_DAY=150.0 \
           -e ENGINE_DEFAULT_CAP=2000.0 \
           your-image
```

## Important Notes

1. **Array Values**: For `ENGINE_DEFAULT_THRESHOLDS` and `ENGINE_DEFAULT_RATES`, use comma-separated values without spaces.
2. **Bracket Consistency**: The number of rates should be exactly one more than the number of thresholds (rates include the final bracket).
3. **Decimal Format**: Use decimal notation (e.g., 0.03 for 3%) for rates and percentages.
4. **Simplified API**: The tool APIs no longer accept rate, cap, interest, threshold, or surcharge parameters. All these values are configured exclusively through environment variables.

## Example Tool Calls

The tool APIs are now simplified - only the essential input parameters are required:

### Penalty Calculation
```json
{
  "name": "calc_penalty",
  "arguments": {
    "days_late": 15.0
  }
}
```

The system automatically uses the configured penalty rate, cap, and interest rate from environment variables.

### Tax Calculation
```json
{
  "name": "calc_tax",
  "arguments": {
    "income": 50000.0
  }
}
```

The system automatically uses the configured tax brackets, rates, surcharge threshold, and surcharge rate from environment variables.
