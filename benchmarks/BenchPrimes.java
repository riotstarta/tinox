public class BenchPrimes {
    public static void main(String[] args) {
        int limit = 1_000_000;
        int count = 0;
        for (int i = 2; i <= limit; i++) {
            boolean isPrime = true;
            for (int j = 2; (long)j * j <= i; j++) {
                if (i % j == 0) {
                    isPrime = false;
                    break;
                }
            }
            if (isPrime) count++;
        }
        System.out.println("primes up to 1000000: " + count);
    }
}
