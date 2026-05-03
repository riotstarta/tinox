public class BenchFib {
    static long fib(long n) {
        if (n <= 1) return n;
        return fib(n - 1) + fib(n - 2);
    }

    public static void main(String[] args) {
        long result = fib(40);
        System.out.println("fib(40) = " + result);
    }
}
